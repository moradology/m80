//! Run-root walk commands: inspect and list.
//!
//! These commands don't go through a full `Backend` admission path —
//! they walk `<run_root>/` on disk to discover VM state.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::Context;

use m80_firecracker::{FcError, OWNERSHIP_LOCK};
use m80_jailer::{JAILER_PLAN_FILE, JAILER_STATE_FILE};

use crate::config;
use crate::errors;
use crate::json;

#[derive(serde::Serialize)]
struct InspectOutput {
    vm_id: String,
    run_dir: PathBuf,
    files: Vec<String>,
    contents: HashMap<String, serde_json::Value>,
}

#[derive(serde::Serialize)]
struct ListEntry {
    vm_id: String,
    state: &'static str,
    run_dir: PathBuf,
}

// =====================================================================
// inspect
// =====================================================================

/// `m80 inspect` — print a VM's run-dir layout and recorded state.
pub fn cmd_inspect(vm_id: &str, json: bool) -> anyhow::Result<i32> {
    let run_root = match effective_run_root() {
        Ok(run_root) => run_root,
        Err(e) => return Ok(errors::render_error(&e, json)),
    };
    let output = match inspect_output(&run_root, vm_id) {
        Ok(output) => output,
        Err(e) => return Ok(errors::render_error(&e, json)),
    };

    if json {
        println!("{}", render_inspect_json(&output));
    } else {
        print!("{}", render_inspect_human(&output));
    }

    Ok(0)
}

fn inspect_output(run_root: &Path, vm_id: &str) -> Result<InspectOutput, FcError> {
    let vm_dir = run_root.join(vm_id);

    if !vm_dir.exists() {
        return Err(FcError::Config(format!(
            "no run-dir found for vm_id={vm_id}"
        )));
    }

    // Files the orchestrator + dependent crates write into <run_dir>/.
    // boot-identity.json is reserved for the v0.2 snapshot manifest path.
    let known_files = &[
        OWNERSHIP_LOCK,
        JAILER_STATE_FILE,
        JAILER_PLAN_FILE,
        "cgroup-path.txt",
        "boot-identity.json",
    ];

    let mut files = Vec::new();
    let mut contents = HashMap::new();

    for name in known_files {
        let path = vm_dir.join(name);
        if path.exists() {
            files.push(name.to_string());
            if name.ends_with(".json") {
                if let Ok(text) = std::fs::read_to_string(&path) {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                        contents.insert(name.to_string(), v);
                    }
                }
            }
        }
    }

    Ok(InspectOutput {
        vm_id: vm_id.to_owned(),
        run_dir: vm_dir,
        files,
        contents,
    })
}

fn render_inspect_json(output: &InspectOutput) -> String {
    json::to_pretty(output)
}

fn render_inspect_human(output: &InspectOutput) -> String {
    let mut out = String::new();
    out.push_str(&format!("vm_id:    {}\n", output.vm_id));
    out.push_str(&format!("run_dir:  {}\n", output.run_dir.display()));
    out.push_str("files:\n");
    for name in &output.files {
        let detail = output
            .contents
            .get(name)
            .map(|v| format!("  {v}"))
            .unwrap_or_default();
        out.push_str(&format!("  {name}{detail}\n"));
    }
    if output.files.is_empty() {
        out.push_str("  (empty)\n");
    }
    out
}

// =====================================================================
// list
// =====================================================================

/// `m80 list` — enumerate VM run-dirs under the run-root.
pub fn cmd_list(json: bool) -> anyhow::Result<i32> {
    let run_root = match effective_run_root() {
        Ok(run_root) => run_root,
        Err(e) => return Ok(errors::render_error(&e, json)),
    };

    if !run_root.exists() {
        if json {
            println!("{}", json::to_pretty(&Vec::<ListEntry>::new()));
        } else {
            println!("(no run-root at {})", run_root.display());
        }
        return Ok(0);
    }

    let entries = match list_entries(&run_root) {
        Ok(entries) => entries,
        Err(e) => return Ok(errors::render_error(&e, json)),
    };

    if json {
        println!("{}", render_list_json(&entries));
    } else {
        print!("{}", render_list_human(&run_root, &entries));
    }

    Ok(0)
}

fn list_entries(run_root: &Path) -> Result<Vec<ListEntry>, FcError> {
    let read_dir = std::fs::read_dir(run_root)
        .with_context(|| format!("listing {}", run_root.display()))
        .map_err(|e| FcError::Config(format!("{e:#}")))?;

    let mut entries = Vec::new();
    for entry in read_dir.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        // Subdirs of the run-root are always vm-id named (we created them);
        // a name that isn't valid UTF-8 means the run-root has been mutated
        // by something outside m80, so skip rather than render `?`.
        let Some(vm_id) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        // "live" iff ownership.lock exists AND the recorded pid is alive.
        // jailer-state.json is written at materialize time and persists
        // across stop, so don't use it as a liveness proxy.
        let state = if vm_dir_is_live(&path) {
            "live"
        } else {
            "stale"
        };
        entries.push(ListEntry {
            vm_id: vm_id.to_owned(),
            state,
            run_dir: path,
        });
    }
    Ok(entries)
}

fn render_list_json(entries: &[ListEntry]) -> String {
    json::to_pretty(entries)
}

fn render_list_human(run_root: &Path, entries: &[ListEntry]) -> String {
    let mut out = String::new();
    if entries.is_empty() {
        out.push_str(&format!("(no VMs in {})\n", run_root.display()));
    }
    for e in entries {
        out.push_str(&format!(
            "{}  [{}]  {}\n",
            e.vm_id,
            e.state,
            e.run_dir.display()
        ));
    }
    out
}

// =====================================================================
// Helpers
// =====================================================================

/// Return the run-root resolved through the same config chain as the backend,
/// without running preflight.
fn effective_run_root() -> Result<PathBuf, FcError> {
    config::resolve_run_root().map_err(|e| FcError::Config(format!("{e:#}")))
}

/// Return `true` iff `<vm_dir>/ownership.lock` records a still-running pid.
fn vm_dir_is_live(vm_dir: &Path) -> bool {
    let lock = vm_dir.join(OWNERSHIP_LOCK);
    let Ok(text) = std::fs::read_to_string(&lock) else {
        return false;
    };
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("pid=") {
            if let Ok(pid) = rest.trim().parse::<u32>() {
                return Path::new(&format!("/proc/{pid}")).exists();
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_vm(run_root: &Path, vm_id: &str, lock_pid: Option<u32>) -> PathBuf {
        let vm_dir = run_root.join(vm_id);
        std::fs::create_dir_all(&vm_dir).unwrap();
        if let Some(pid) = lock_pid {
            std::fs::write(vm_dir.join(OWNERSHIP_LOCK), format!("pid={pid}\n")).unwrap();
        }
        std::fs::write(
            vm_dir.join(JAILER_STATE_FILE),
            r#"{"firecracker_pid":1234}"#,
        )
        .unwrap();
        std::fs::write(vm_dir.join(JAILER_PLAN_FILE), r#"{"jailer":"fixture"}"#).unwrap();
        std::fs::write(
            vm_dir.join("cgroup-path.txt"),
            "/sys/fs/cgroup/m80/fixture\n",
        )
        .unwrap();
        vm_dir
    }

    #[test]
    fn list_entries_classifies_live_only_from_ownership_pid() {
        let temp = tempfile::tempdir().unwrap();
        write_vm(temp.path(), "live-vm", Some(std::process::id()));
        write_vm(temp.path(), "stale-vm", None);

        let entries = list_entries(temp.path()).unwrap();
        let live = entries
            .iter()
            .find(|entry| entry.vm_id == "live-vm")
            .unwrap();
        let stale = entries
            .iter()
            .find(|entry| entry.vm_id == "stale-vm")
            .unwrap();

        assert_eq!(live.state, "live");
        assert_eq!(stale.state, "stale");
    }

    #[test]
    fn list_json_and_human_render_fixture_entries() {
        let temp = tempfile::tempdir().unwrap();
        write_vm(temp.path(), "live-vm", Some(std::process::id()));
        write_vm(temp.path(), "stale-vm", None);

        let entries = list_entries(temp.path()).unwrap();
        let json: serde_json::Value = serde_json::from_str(&render_list_json(&entries)).unwrap();
        assert_eq!(json["version"], 1);
        assert_eq!(json["data"].as_array().unwrap().len(), 2);

        let human = render_list_human(temp.path(), &entries);
        assert!(human.contains("live-vm  [live]"));
        assert!(human.contains("stale-vm  [stale]"));
    }

    #[test]
    fn inspect_json_and_human_render_known_run_root_files() {
        let temp = tempfile::tempdir().unwrap();
        let vm_dir = write_vm(temp.path(), "inspect-vm", Some(std::process::id()));

        let output = inspect_output(temp.path(), "inspect-vm").unwrap();
        assert_eq!(output.run_dir, vm_dir);
        assert!(output.files.contains(&OWNERSHIP_LOCK.to_owned()));
        assert!(output.files.contains(&JAILER_STATE_FILE.to_owned()));
        assert!(output.files.contains(&JAILER_PLAN_FILE.to_owned()));
        assert!(output.files.contains(&"cgroup-path.txt".to_owned()));

        let json: serde_json::Value = serde_json::from_str(&render_inspect_json(&output)).unwrap();
        assert_eq!(json["version"], 1);
        assert_eq!(json["data"]["vm_id"], "inspect-vm");
        assert_eq!(
            json["data"]["contents"][JAILER_STATE_FILE]["firecracker_pid"],
            1234
        );

        let human = render_inspect_human(&output);
        assert!(human.contains("vm_id:    inspect-vm"));
        assert!(human.contains(JAILER_STATE_FILE));
        assert!(human.contains("firecracker_pid"));
    }
}
