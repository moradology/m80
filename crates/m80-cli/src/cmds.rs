//! Lifecycle subcommand implementations for `m80`.
//!
//! Covers: preflight, launch, exec, cleanup, config show, version.
//! Walk-based commands (stop, inspect, list) live in `cmds_walk`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;

use m80_firecracker::ExecRequest;
use m80_firecracker::{Backend, EffectiveConfig, FcError, NetworkPolicy, SandboxConfig, SnapshotPaths};

use crate::config;
use crate::errors;

/// Run preflight and build an `Arc<Backend>`. Returns `EffectiveConfig`
/// alongside so `config show` can reuse the merge result without rerunning.
pub(crate) fn build_backend(
    flag_overrides: &HashMap<&str, String>,
) -> Result<(Arc<Backend>, EffectiveConfig), FcError> {
    let discovery = m80_preflight::run()?;
    let (backend_config, effective) =
        config::load(discovery, flag_overrides).map_err(|e| FcError::Config(format!("{e:#}")))?;
    let backend = Backend::new(backend_config)?;
    Ok((Arc::new(backend), effective))
}

/// `m80 preflight` — run host capability checks and render a table.
pub fn cmd_preflight(json: bool) -> anyhow::Result<i32> {
    match m80_preflight::run() {
        Ok(discovery) => {
            if json {
                let rows = &discovery.report;
                let j = serde_json::to_string_pretty(rows).expect("preflight report serialization");
                println!("{j}");
            } else {
                println!("{}", discovery.render_table());
            }
            Ok(0)
        }
        Err(e) => {
            let fc_err = m80_firecracker::FcError::Preflight(e);
            Ok(errors::render_error(&fc_err, json))
        }
    }
}

/// `m80 launch` — boot a VM (cold or from snapshot).
///
/// Empty `exec_argv` blocks until SIGINT; non-empty runs single-shot exec
/// then stops the VM, returning the guest's exit code.
///
/// When `from_snapshot` is `Some`, restores from a previously captured
/// snapshot directory (must contain `vm.snap` and `mem.snap`) instead of
/// performing a cold boot.
pub fn cmd_launch(
    workspace: Option<PathBuf>,
    network: NetworkPolicy,
    id: Option<String>,
    from_snapshot: Option<PathBuf>,
    exec_argv: Vec<String>,
    json: bool,
) -> anyhow::Result<i32> {
    let (backend, _effective) = match build_backend(&HashMap::new()) {
        Ok(pair) => pair,
        Err(e) => return Ok(errors::render_error(&e, json)),
    };

    let sandbox_config = SandboxConfig {
        vm_id: id,
        workspace,
        network,
        vcpu_count: None,
        mem_size_mib: None,
        boot_args: None,
        overlay_size_bytes: 512 * 1024 * 1024,
        idle_timeout: None,
    };

    let sandbox = match backend.admit(sandbox_config) {
        Ok(s) => s,
        Err(e) => {
            return Ok(errors::render_error(&e, json));
        }
    };

    let mut running = if let Some(snap_dir) = from_snapshot {
        // Restore path: verify snapshot files exist before calling into the
        // orchestrator (fail early with a clear message rather than letting
        // Firecracker emit a cryptic ENOENT from inside the jailer).
        let vm_snap = snap_dir.join("vm.snap");
        let mem_snap = snap_dir.join("mem.snap");
        if !vm_snap.exists() {
            let e = FcError::Config(format!(
                "snapshot file not found: {}; expected vm.snap and mem.snap in --from-snapshot dir",
                vm_snap.display()
            ));
            return Ok(errors::render_error(&e, json));
        }
        if !mem_snap.exists() {
            let e = FcError::Config(format!(
                "snapshot file not found: {}; expected vm.snap and mem.snap in --from-snapshot dir",
                mem_snap.display()
            ));
            return Ok(errors::render_error(&e, json));
        }
        let snapshot_paths = SnapshotPaths {
            vm_state: vm_snap,
            mem: mem_snap,
        };
        let discovery = m80_preflight::run()
            .map_err(FcError::Preflight)?;
        match sandbox.launch_from_snapshot(snapshot_paths, &discovery) {
            Ok(r) => r,
            Err(e) => return Ok(errors::render_error(&e, json)),
        }
    } else {
        // Cold-boot path.
        match sandbox.launch() {
            Ok(r) => r,
            Err(e) => return Ok(errors::render_error(&e, json)),
        }
    };

    let vm_id = running.vm_id().to_owned();

    if json {
        let obj = serde_json::json!({ "vm_id": vm_id });
        println!("{}", serde_json::to_string_pretty(&obj).unwrap());
    } else {
        println!("vm_id: {vm_id}");
    }

    if exec_argv.is_empty() {
        // SIGINT unwinds the stack, which runs RunningSandbox's Drop chain
        // (vsock close, cgroup rm, jailer umount, permit release). The
        // 1-hour sleep is just polite parking — the signal interrupts it.
        eprintln!("VM launched. Press Ctrl-C to stop.");
        loop {
            std::thread::sleep(Duration::from_secs(3600));
        }
    }

    // Single-shot mode: split argv into program + args and exec.
    let mut argv = exec_argv;
    let program = argv.remove(0);
    let req = ExecRequest {
        program,
        args: argv,
        env: None,
        cwd: None,
        stdin: None,
        timeout_ms: None,
    };

    let response = match running.exec(req) {
        Ok(r) => r,
        Err(e) => {
            let _ = running.force_kill();
            return Ok(errors::render_error(&e, json));
        }
    };

    if json {
        let j = serde_json::to_string_pretty(&response).expect("exec response serialization");
        println!("{j}");
    } else {
        if !response.stdout.is_empty() {
            print!("{}", String::from_utf8_lossy(&response.stdout));
        }
        if !response.stderr.is_empty() {
            eprint!("{}", String::from_utf8_lossy(&response.stderr));
        }
    }

    let guest_exit = response.exit_code.unwrap_or(1);

    let stopped = match running.stop() {
        Ok(s) => s,
        Err(e) => {
            return Ok(errors::render_error(&e, json));
        }
    };

    if let Err(e) = stopped.delete() {
        eprintln!("warning: delete failed: {e}");
    }

    Ok(guest_exit)
}

/// `m80 snapshot capture` — v0.1 stub.
///
/// Capturing a running VM requires the caller to hold the `RunningSandbox`
/// handle in-process (same gap as `m80 exec` — out-of-process IPC is v0.2).
/// For library use, call `RunningSandbox::capture()` directly and pass in
/// a [`m80_firecracker::SnapshotPaths`] pointing at your chosen directory.
///
/// Example (library path):
/// ```text
/// let paths = SnapshotPaths { vm_state: dir.join("vm.snap"), mem: dir.join("mem.snap") };
/// running.capture(paths)?;
/// ```
pub fn cmd_snapshot_capture(vm_id: &str, _store_root: &Path, json: bool) -> anyhow::Result<i32> {
    let msg = format!(
        "v0.1 limitation: `m80 snapshot capture` requires out-of-process IPC (planned for v0.2). \
         Capture is only available via RunningSandbox::capture() when the VM is launched \
         in the same process. For single-shot capture, embed m80-firecracker and call \
         capture() directly. vm_id={vm_id}"
    );
    if json {
        let obj = serde_json::json!({
            "error": msg,
            "exit_code": errors::EXIT_NOT_IMPLEMENTED,
        });
        eprintln!("{}", serde_json::to_string_pretty(&obj).unwrap());
    } else {
        eprintln!("error: {msg}");
    }
    Ok(errors::EXIT_NOT_IMPLEMENTED)
}

/// `m80 exec` — v0.1 stub.
///
/// Out-of-process exec requires a side-channel IPC mechanism (v0.2).
/// Use `m80 launch -- <argv>` for single-shot launch+exec in v0.1, or
/// embed `m80-firecracker` and call `RunningSandbox::exec()` directly.
pub fn cmd_exec(vm_id: &str, _argv: &[String], json: bool) -> anyhow::Result<i32> {
    let msg = format!(
        "v0.1 limitation: `m80 exec` requires out-of-process IPC (planned for v0.2). \
         Use `m80 launch -- <argv>` for single-shot launch+exec. \
         For library use, call RunningSandbox::exec() directly. \
         vm_id={vm_id}"
    );
    if json {
        let obj = serde_json::json!({
            "error": msg,
            "exit_code": errors::EXIT_NOT_IMPLEMENTED,
        });
        eprintln!("{}", serde_json::to_string_pretty(&obj).unwrap());
    } else {
        eprintln!("error: {msg}");
    }
    Ok(errors::EXIT_NOT_IMPLEMENTED)
}

/// `m80 cleanup` — trigger `recover_stale_run_root()`.
pub fn cmd_cleanup(_force: bool, json: bool) -> anyhow::Result<i32> {
    let (backend, _effective) = match build_backend(&HashMap::new()) {
        Ok(pair) => pair,
        Err(e) => return Ok(errors::render_error(&e, json)),
    };

    if let Err(e) = backend.recover_stale_run_root() {
        return Ok(errors::render_error(&e, json));
    }

    if json {
        let obj = serde_json::json!({ "status": "ok" });
        println!("{}", serde_json::to_string_pretty(&obj).unwrap());
    } else {
        println!("cleanup complete");
    }

    Ok(0)
}

/// `m80 config show` — reveal the merged effective config with field sources.
pub fn cmd_config_show(json: bool) -> anyhow::Result<i32> {
    let discovery = m80_preflight::run().context("preflight failed")?;
    let (_, effective) = config::load(discovery, &HashMap::new())?;

    if json {
        let j = serde_json::to_string_pretty(&effective).expect("effective config serialization");
        println!("{j}");
    } else {
        println!("{:<25} {:<20} SOURCE", "FIELD", "VALUE");
        println!("{}", "-".repeat(60));
        for field in &effective.fields {
            println!("{:<25} {:<20} {:?}", field.name, field.value, field.source);
        }
    }

    Ok(0)
}

/// `m80 version` — print version strings.
///
/// Reports binary version, wire-protocol version, and (best-effort) the
/// Firecracker version pin from the manifest beside `M80_ROOTFS_IMAGE`.
/// If the manifest can't be read, the Firecracker pin renders as
/// `"unknown"` rather than failing the subcommand.
pub fn cmd_version(json: bool) -> anyhow::Result<i32> {
    let binary_version = env!("CARGO_PKG_VERSION");
    let protocol_version = m80_proto::PROTOCOL_VERSION;
    let firecracker_pin = read_firecracker_pin();

    if json {
        let obj = serde_json::json!({
            "binary_version": binary_version,
            "protocol_version": protocol_version,
            "firecracker_pin": firecracker_pin,
        });
        println!("{}", serde_json::to_string_pretty(&obj).unwrap());
    } else {
        println!("m80           {binary_version}");
        println!("protocol      {protocol_version}");
        println!("firecracker   {firecracker_pin}");
    }

    Ok(0)
}

/// Best-effort read of the Firecracker version pin from the manifest beside
/// `M80_ROOTFS_IMAGE`. Returns `"unknown"` when the env var is unset or the
/// manifest can't be parsed (this subcommand must never fail).
fn read_firecracker_pin() -> String {
    let Ok(rootfs) = std::env::var("M80_ROOTFS_IMAGE") else {
        return "unknown (set M80_ROOTFS_IMAGE)".to_string();
    };
    let manifest_path = format!("{rootfs}.manifest.json");
    match m80_image_manifest::Manifest::read(std::path::Path::new(&manifest_path)) {
        Ok(m) => m.expected_firecracker_version,
        Err(_) => "unknown".to_string(),
    }
}
