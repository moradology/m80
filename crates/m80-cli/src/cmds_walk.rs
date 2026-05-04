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

use m80_firecracker::OWNERSHIP_LOCK;
use m80_jailer::{JAILER_PLAN_FILE, JAILER_STATE_FILE};

use crate::cmds::build_backend;
use crate::config;
use crate::errors;

// =====================================================================
// stop
// =====================================================================

/// `m80 stop` — walk the run-root, kill recorded pids, recover residue.
///
/// v0.1: best-effort force-stop by reading `jailer-state.json` from the
/// VM's run-dir and SIGKILLing the recorded pids. Clean stop via IPC is
/// v0.2.
pub fn cmd_stop(vm_id: &str, extract_changes: Option<&Path>, json: bool) -> anyhow::Result<i32> {
    let (backend, _effective) = match build_backend(&HashMap::new()) {
        Ok(pair) => pair,
        Err(e) => {
            eprintln!("error: {e}");
            return Ok(errors::EXIT_GENERIC);
        }
    };

    let run_root = backend.config().run_root.clone();
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

    // Remove the ownership.lock so future recover_stale_run_root passes can
    // reclaim the run-dir even if recovery races with a re-spawned VM.
    let lock = vm_dir.join(OWNERSHIP_LOCK);
    if lock.exists() {
        if let Err(e) = std::fs::remove_file(&lock) {
            eprintln!("warning: failed to remove {}: {e}", lock.display());
        }
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

/// SIGKILL the firecracker pid recorded in `<vm_dir>/jailer-state.json`.
/// Returns 1 on success, 0 if the file is absent / unparseable / kill
/// failed. `jailer_pid` is not signalled separately — jailer execs into
/// firecracker so the two pids are equal.
fn kill_jailer_pids(vm_dir: &Path) -> usize {
    let jailer_state = vm_dir.join(JAILER_STATE_FILE);
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

    // m80-jailer writes top-level scalar fields, not a `pids` array.
    let Some(pid) = parsed.get("firecracker_pid").and_then(|v| v.as_i64()) else {
        return 0;
    };

    let raw = nix::unistd::Pid::from_raw(pid as i32);
    if nix::sys::signal::kill(raw, nix::sys::signal::Signal::SIGKILL).is_ok() {
        1
    } else {
        0
    }
}

// =====================================================================
// inspect
// =====================================================================

/// `m80 inspect` — print a VM's run-dir layout and recorded state.
pub fn cmd_inspect(vm_id: &str, json: bool) -> anyhow::Result<i32> {
    let run_root = effective_run_root();
    let vm_dir = run_root.join(vm_id);

    if !vm_dir.exists() {
        eprintln!("error: no run-dir found for vm_id={vm_id}");
        return Ok(errors::EXIT_GENERIC);
    }

    // Files the orchestrator + dependent crates write into <run_dir>/.
    // boot-identity.json is reserved for the v0.2 snapshot manifest path
    // (m80-snapshot writes it as part of the capture lane); not present
    // in v0.1 — listed so consumers know to expect it once v0.2 lands.
    let known_files = &[
        OWNERSHIP_LOCK,
        JAILER_STATE_FILE,
        JAILER_PLAN_FILE,
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
    let run_root = effective_run_root();

    if !run_root.exists() {
        if json {
            println!("[]");
        } else {
            println!("(no run-root at {})", run_root.display());
        }
        return Ok(0);
    }

    #[derive(serde::Serialize)]
    struct Entry {
        vm_id: String,
        state: &'static str,
        run_dir: PathBuf,
    }

    let mut entries: Vec<Entry> = Vec::new();

    let read_dir =
        std::fs::read_dir(&run_root).with_context(|| format!("listing {}", run_root.display()))?;

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
        entries.push(Entry {
            vm_id: vm_id.to_owned(),
            state,
            run_dir: path,
        });
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
            println!("{}  [{}]  {}", e.vm_id, e.state, e.run_dir.display());
        }
    }

    Ok(0)
}

// =====================================================================
// Helpers
// =====================================================================

/// Return the run-root resolved through the same config chain
/// (`build_backend` uses internally): defaults → `/etc/m80/config.toml` →
/// `~/.config/m80/config.toml` → `M80_*` env. We deliberately don't run
/// preflight here (the read-only walk commands shouldn't pay for it); fall
/// back to the env+default if the config probe fails.
fn effective_run_root() -> PathBuf {
    if let Ok(rr) = config::resolve_run_root() {
        return rr;
    }
    std::env::var("M80_RUN_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/var/run/m80"))
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
