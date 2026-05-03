//! Lifecycle subcommand implementations for `m80`.
//!
//! Covers: preflight, launch, exec, cleanup, config show, version.
//! Walk-based commands (stop, inspect, list) live in `cmds_walk`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;

use m80_firecracker::{Backend, EffectiveConfig, NetworkPolicy, SandboxConfig};
use m80_proto::ExecRequest;

use crate::config;
use crate::errors;

// =====================================================================
// Backend construction helper
// =====================================================================

/// Run preflight and build an `Arc<Backend>` from the merged config.
///
/// Returns both the backend and the tagged `EffectiveConfig` produced by
/// the merge so `config show` can display provenance without re-running.
pub fn build_backend(
    flag_overrides: &HashMap<&str, String>,
) -> anyhow::Result<(Arc<Backend>, EffectiveConfig)> {
    let discovery = m80_preflight::run().context("preflight failed")?;
    let (backend_config, effective) = config::load(discovery, flag_overrides)?;
    let backend =
        Backend::new(backend_config).map_err(|e| anyhow::anyhow!("backend init: {e}"))?;
    Ok((Arc::new(backend), effective))
}

// =====================================================================
// preflight
// =====================================================================

/// `m80 preflight` — run host capability checks and render a table.
pub fn cmd_preflight(json: bool) -> anyhow::Result<i32> {
    match m80_preflight::run() {
        Ok(discovery) => {
            if json {
                let rows = &discovery.report;
                let j = serde_json::to_string_pretty(rows)
                    .expect("preflight report serialization");
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

// =====================================================================
// launch
// =====================================================================

/// `m80 launch` — boot a VM, then either run single-shot exec or block.
///
/// When `exec_argv` is non-empty this is single-shot mode: launch the VM,
/// run the argv, print output, stop the VM, and exit with the guest's
/// exit code.
///
/// When `exec_argv` is empty, launch blocks until SIGINT (Ctrl-C).
pub fn cmd_launch(
    workspace: Option<PathBuf>,
    network: NetworkPolicy,
    id: Option<String>,
    exec_argv: Vec<String>,
    json: bool,
) -> anyhow::Result<i32> {
    let (backend, _effective) = match build_backend(&HashMap::new()) {
        Ok(pair) => pair,
        Err(e) => {
            eprintln!("error: {e}");
            return Ok(errors::EXIT_GENERIC);
        }
    };

    let sandbox_config = SandboxConfig {
        vm_id: id,
        workspace,
        network,
        vcpu_count: None,
        mem_size_mib: None,
        boot_args: None,
        default_exec_timeout: None,
    };

    let sandbox = match backend.admit(sandbox_config) {
        Ok(s) => s,
        Err(e) => {
            return Ok(errors::render_error(&e, json));
        }
    };

    let mut running = match sandbox.launch() {
        Ok(r) => r,
        Err(e) => {
            return Ok(errors::render_error(&e, json));
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
        // Block until Ctrl-C. The Drop chain cleans up on process exit.
        eprintln!("VM launched. Press Ctrl-C to stop.");
        loop {
            std::thread::sleep(Duration::from_secs(3600));
        }
    }

    // Single-shot mode: split argv into program + args and exec.
    let (program, args) = split_argv(exec_argv);
    let req = ExecRequest {
        program,
        args,
        env: None,
        cwd: None,
        stdin: None,
        workspace_dir: None,
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

/// Split a full argv into `(program, args)`.
fn split_argv(mut argv: Vec<String>) -> (String, Vec<String>) {
    if argv.is_empty() {
        return (String::new(), vec![]);
    }
    let program = argv.remove(0);
    (program, argv)
}

// =====================================================================
// exec (v0.1 stub)
// =====================================================================

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
        let obj = serde_json::json!({ "error": msg, "exit_code": 5 });
        eprintln!("{}", serde_json::to_string_pretty(&obj).unwrap());
    } else {
        eprintln!("error: {msg}");
    }
    Ok(5)
}

// =====================================================================
// cleanup
// =====================================================================

/// `m80 cleanup` — trigger `recover_stale_run_root()`.
pub fn cmd_cleanup(_force: bool, json: bool) -> anyhow::Result<i32> {
    let (backend, _effective) = match build_backend(&HashMap::new()) {
        Ok(pair) => pair,
        Err(e) => {
            eprintln!("error: {e}");
            return Ok(errors::EXIT_GENERIC);
        }
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

// =====================================================================
// config show
// =====================================================================

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

// =====================================================================
// version
// =====================================================================

/// `m80 version` — print version strings.
pub fn cmd_version(json: bool) -> anyhow::Result<i32> {
    let binary_version = env!("CARGO_PKG_VERSION");
    let protocol_version = m80_proto::PROTOCOL_VERSION;

    if json {
        let obj = serde_json::json!({
            "binary_version": binary_version,
            "protocol_version": protocol_version,
        });
        println!("{}", serde_json::to_string_pretty(&obj).unwrap());
    } else {
        println!("m80         {binary_version}");
        println!("protocol    {protocol_version}");
    }

    Ok(0)
}
