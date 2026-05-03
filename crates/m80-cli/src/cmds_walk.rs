//! Run-root walk commands: stop, inspect, list.
//!
//! These commands don't go through a full `Backend` admission path —
//! they walk `<run_root>/` on disk to discover VM state.
//!
//! `stop` additionally SIGKILLs pids from `jailer-state.json` and
//! calls `Backend::recover_stale_run_root()` to clean up residue.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::Context;

use m80_firecracker::Backend;

use crate::cmds::build_backend;
use crate::errors;

// =====================================================================
// stop
// =====================================================================

/// `m80 stop` — walk the run-root, kill recorded pids, recover residue.
///
/// v0.1: best-effort force-stop by reading `jailer-state.json` from the
/// VM's run-dir and SIGKILLing the recorded pids. Clean stop via IPC is
/// v0.2.
pub fn cmd_stop(
    vm_id: &str,
    extract_changes: Option<&Path>,
    json: bool,
) -> anyhow::Result<i32> {
    let (backend, _effective) = match build_backend(&HashMap::new()) {
        Ok(pair) => pair,
        Err(e) => {
            eprintln!("error: {e}");
            return Ok(errors::EXIT_GENERIC);
        }
    };

    let run_root = backend_run_root(&backend);
    let vm_dir = run_root.join(vm_id);

    if !vm_dir.exists() {
        eprintln!("error: no run-dir found for vm_id={vm_id}");
        return Ok(errors::EXIT_GENERIC);
    }

    let killed = kill_jailer_pids(&vm_dir);

    if let Some(dest) = extract_changes {
        eprintln!(
            "note: change extraction (--extract-changes) requires a RunningSandbox handle \
             (v0.2 feature); dest={} will not be populated",
            dest.display()
        );
    }

    if let Err(e) = backend.recover_stale_run_root() {
        eprintln!("warning: recover_stale_run_root failed: {e}");
    }

    if json {
        let obj = serde_json::json!({
            "vm_id": vm_id,
            "pids_killed": killed,
        });
        println!("{}", serde_json::to_string_pretty(&obj).unwrap());
    } else {
        println!(
            "stopped vm_id={vm_id} (killed {} pids, recovery scan triggered)",
            killed
        );
    }

    Ok(0)
}

/// Attempt to SIGKILL all pids recorded in `<vm_dir>/jailer-state.json`.
/// Returns the number of successfully-signalled pids.
fn kill_jailer_pids(vm_dir: &Path) -> usize {
    let jailer_state = vm_dir.join("jailer-state.json");
    if !jailer_state.exists() {
        return 0;
    }

    let text = match std::fs::read_to_string(&jailer_state) {
        Ok(t) => t,
        Err(_) => return 0,
    };

    let parsed: serde_json::Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(_) => return 0,
    };

    let pids = match parsed.get("pids").and_then(|p| p.as_array()) {
        Some(arr) => arr.clone(),
        None => return 0,
    };

    let mut killed = 0usize;
    for pid_val in &pids {
        if let Some(pid) = pid_val.as_i64() {
            let raw = nix::unistd::Pid::from_raw(pid as i32);
            if nix::sys::signal::kill(raw, nix::sys::signal::Signal::SIGKILL).is_ok() {
                killed += 1;
            }
        }
    }
    killed
}

// =====================================================================
// inspect
// =====================================================================

/// `m80 inspect` — print a VM's run-dir layout and recorded state.
pub fn cmd_inspect(vm_id: &str, json: bool) -> anyhow::Result<i32> {
    let run_root = run_root_from_env();
    let vm_dir = run_root.join(vm_id);

    if !vm_dir.exists() {
        eprintln!("error: no run-dir found for vm_id={vm_id}");
        return Ok(errors::EXIT_GENERIC);
    }

    let known_files = &[
        "ownership.lock",
        "jailer-state.json",
        "cgroup-path.txt",
        "boot-identity.json",
    ];

    let mut present: Vec<String> = Vec::new();
    let mut contents: HashMap<&str, serde_json::Value> = HashMap::new();

    for name in known_files {
        let path = vm_dir.join(name);
        if path.exists() {
            present.push(name.to_string());
            if name.ends_with(".json") {
                if let Ok(text) = std::fs::read_to_string(&path) {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                        contents.insert(name, v);
                    }
                }
            }
        }
    }

    if json {
        let obj = serde_json::json!({
            "vm_id": vm_id,
            "run_dir": vm_dir,
            "files": present,
            "contents": contents,
        });
        println!("{}", serde_json::to_string_pretty(&obj).unwrap());
    } else {
        println!("vm_id:    {vm_id}");
        println!("run_dir:  {}", vm_dir.display());
        println!("files:");
        for name in &present {
            let detail = contents
                .get(name.as_str())
                .map(|v| format!("  {v}"))
                .unwrap_or_default();
            println!("  {name}{detail}");
        }
        if present.is_empty() {
            println!("  (empty)");
        }
    }

    Ok(0)
}

// =====================================================================
// list
// =====================================================================

/// `m80 list` — enumerate VM run-dirs under the run-root.
pub fn cmd_list(json: bool) -> anyhow::Result<i32> {
    let run_root = run_root_from_env();

    if !run_root.exists() {
        if json {
            println!("[]");
        } else {
            println!("(no run-root at {})", run_root.display());
        }
        return Ok(0);
    }

    let mut entries: Vec<serde_json::Value> = Vec::new();

    let read_dir = std::fs::read_dir(&run_root)
        .with_context(|| format!("listing {}", run_root.display()))?;

    for entry in read_dir.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let vm_id = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("?")
            .to_owned();

        let state = if path.join("jailer-state.json").exists() {
            "stale"
        } else {
            "live"
        };

        entries.push(serde_json::json!({ "vm_id": vm_id, "state": state, "run_dir": path }));
    }

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&entries).expect("list serialization")
        );
    } else {
        if entries.is_empty() {
            println!("(no VMs in {})", run_root.display());
        }
        for e in &entries {
            let vm_id = e["vm_id"].as_str().unwrap_or("?");
            let state = e["state"].as_str().unwrap_or("?");
            let dir = e["run_dir"].as_str().unwrap_or("?");
            println!("{vm_id}  [{state}]  {dir}");
        }
    }

    Ok(0)
}

// =====================================================================
// Helpers
// =====================================================================

/// Return the run-root from `M80_RUN_ROOT` env or the compiled default.
fn run_root_from_env() -> PathBuf {
    std::env::var("M80_RUN_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/var/run/m80"))
}

/// Extract the `run_root` from a constructed `Backend`.
fn backend_run_root(backend: &Backend) -> PathBuf {
    backend.config.run_root.clone()
}
