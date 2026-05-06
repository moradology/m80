use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead as _, BufReader, Read as _};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use m80_firecracker::{Backend, EffectiveConfig, FcError, NetworkPolicy, SandboxConfig};
use m80_preflight::{CheckRow, Discovery, PreflightError};

use crate::args::{EgressMode, QuickstartArgs, WarmAction, WritebackMode};
use crate::config;
use crate::errors;
use crate::json;
use crate::profile::{self, ProfileFilePaths};

/// Resolve config, run preflight, and build an `Arc<Backend>`.
/// Returns `EffectiveConfig` alongside for callers that need source labels.
pub(crate) fn build_backend(
    flag_overrides: &HashMap<&str, String>,
) -> Result<(Arc<Backend>, EffectiveConfig), FcError> {
    let effective =
        config::load_effective(flag_overrides).map_err(|e| FcError::Config(format!("{e:#}")))?;
    backend_from_effective(effective)
}

fn build_run_backend(profile: Option<String>) -> Result<(Arc<Backend>, EffectiveConfig), FcError> {
    let mut flag_overrides = HashMap::new();
    if let Some(profile) = profile {
        flag_overrides.insert("default_profile", profile);
    }
    let effective =
        config::load_effective(&flag_overrides).map_err(|e| FcError::Config(format!("{e:#}")))?;
    let runtime_profile = profile::resolve_from_effective(&effective, ProfileFilePaths::host())?;
    let _profile_env = runtime_profile.apply_env();
    backend_from_effective(effective)
}

fn backend_from_effective(
    effective: EffectiveConfig,
) -> Result<(Arc<Backend>, EffectiveConfig), FcError> {
    let discovery = m80_preflight::run()?;
    let backend_config = config::backend_config(discovery, &effective)
        .map_err(|e| FcError::Config(format!("{e:#}")))?;
    let backend = Backend::new_with_effective_config(backend_config, effective.clone())?;
    Ok((Arc::new(backend), effective))
}

/// `m80 run` — boot a sandbox, run one process, mirror stdout/stderr/exit.
#[allow(clippy::too_many_arguments)]
pub fn cmd_run(
    profile: Option<String>,
    workspace: Option<PathBuf>,
    cwd: Option<String>,
    env: Vec<String>,
    secret_env: Vec<String>,
    stdin: bool,
    egress: EgressMode,
    allow_host: Vec<String>,
    allow_cidr: Vec<String>,
    mount_config: Vec<String>,
    scratch_size: Option<u64>,
    writeback: WritebackMode,
    keep_on_failure: bool,
    tty: bool,
    interactive: bool,
    warm: bool,
    argv: Vec<String>,
    json: bool,
) -> anyhow::Result<i32> {
    let request_id = crate::request_id::new();
    let _request_id_scope = crate::request_id::set(request_id.clone());

    if interactive && !tty {
        let e = FcError::Config("m80 run -i requires --tty / -t".to_owned());
        return Ok(errors::render_error(&e, json));
    }
    if tty && json {
        let e = FcError::Config("m80 run --tty is incompatible with --json".to_owned());
        return Ok(errors::render_error(&e, json));
    }
    if tty && stdin {
        let e = FcError::Config("m80 run --stdin is incompatible with --tty".to_owned());
        return Ok(errors::render_error(&e, json));
    }
    if warm && workspace.is_some() {
        let e = FcError::Config(
            "m80 run --warm --workspace is unsupported until attach-late workspace lands"
                .to_owned(),
        );
        return Ok(errors::render_error(&e, json));
    }
    if warm && tty {
        let e = FcError::Config(
            "m80 run --warm is incompatible with --tty until warm terminal leases land".to_owned(),
        );
        return Ok(errors::render_error(&e, json));
    }
    if !allow_host.is_empty() || !allow_cidr.is_empty() {
        return Ok(render_not_implemented(
            "`m80 run --allow-host` and `--allow-cidr` are reserved for egress allowlists",
            json,
        ));
    }
    if !mount_config.is_empty() {
        return Ok(render_not_implemented(
            "`m80 run --mount-config` is reserved for explicit config-file projection",
            json,
        ));
    }
    if keep_on_failure {
        return Ok(render_not_implemented(
            "`m80 run --keep-on-failure` is reserved for diagnostics retention",
            json,
        ));
    }

    let Some((program, args)) = argv.split_first() else {
        let e = FcError::Config("m80 run requires a command after `--`".to_owned());
        return Ok(errors::render_error(&e, json));
    };
    if scratch_size == Some(0) {
        let e = FcError::Config("m80 run --scratch-size must be greater than zero".to_owned());
        return Ok(errors::render_error(&e, json));
    }
    if writeback != WritebackMode::Never && workspace.is_none() {
        let e = FcError::Config("m80 run --writeback requires --workspace".to_owned());
        return Ok(errors::render_error(&e, json));
    }
    let workspace_for_writeback = workspace.clone();

    let env = match build_process_env(env, secret_env) {
        Ok(env) => env,
        Err(e) => return Ok(errors::render_error(&e, json)),
    };

    let stdin_bytes = if stdin {
        let mut bytes = Vec::new();
        if let Err(e) = std::io::stdin().read_to_end(&mut bytes) {
            return Ok(errors::render_error(&FcError::Io(e), json));
        }
        Some(bytes)
    } else {
        None
    };

    if warm {
        let req = run_request::exec_request_for_run(program, args, env, cwd, stdin_bytes);
        return Ok(warm::cmd_run_warm(profile, egress, req, json));
    }

    let (backend, _effective) = match build_run_backend(profile) {
        Ok(pair) => pair,
        Err(e) => return Ok(errors::render_error(&e, json)),
    };

    let sandbox_config = sandbox_config_for_run(workspace, egress, scratch_size, request_id);

    let sandbox = match backend.admit(sandbox_config) {
        Ok(s) => s,
        Err(e) => return Ok(errors::render_error(&e, json)),
    };

    let mut running = match sandbox.launch() {
        Ok(r) => r,
        Err(e) => return Ok(errors::render_error(&e, json)),
    };

    let guest_exit = if tty {
        let req = run_request::pty_request_for_run(program, args, env, cwd);
        let outcome = match pty::exec_pty_streaming(&mut running, req, interactive) {
            Ok(outcome) => outcome,
            Err(e) => {
                let _ = running.force_kill();
                return Ok(errors::render_error(&e, json));
            }
        };
        pty::pty_exit_code(&outcome.payload, outcome.signal)
    } else if json {
        let req = run_request::exec_request_for_run(program, args, env, cwd, stdin_bytes);
        let outcome = match run_stream::exec_buffered(&mut running, req) {
            Ok(outcome) => outcome,
            Err(e) => {
                let _ = running.force_kill();
                return Ok(errors::render_error(&e, json));
            }
        };
        let response = outcome.payload;
        let exit_code =
            run_stream::process_exit_code(response.status, response.exit_code, outcome.signal);
        println!("{}", json::to_pretty(&response));
        exit_code
    } else {
        let req = run_request::exec_request_for_run(program, args, env, cwd, stdin_bytes);
        let outcome = match run_stream::exec_pipe_streaming(&mut running, req) {
            Ok(outcome) => outcome,
            Err(e) => {
                let _ = running.force_kill();
                return Ok(errors::render_error(&e, json));
            }
        };
        let exit = outcome.payload;
        run_stream::process_exit_code(exit.status, exit.exit_code, outcome.signal)
    };

    emit_guest_boot_trace_if_enabled(running.run_dir());

    let stopped = match running.stop() {
        Ok(s) => s,
        Err(e) => return Ok(errors::render_error(&e, json)),
    };

    if should_writeback(writeback, guest_exit) {
        let workspace = workspace_for_writeback
            .as_ref()
            .expect("writeback policy with no workspace is rejected before launch");
        if let Err(e) = stopped.extract_changes(workspace) {
            match stopped.preserve_for_triage() {
                Ok(path) => render_warning(
                    "writeback_failed_preserved",
                    &format!(
                        "writeback failed; preserved run directory at {}",
                        path.display()
                    ),
                    json,
                ),
                Err(preserve_err) => render_warning(
                    "writeback_failed_preserve_failed",
                    &format!("writeback failed and preserve failed: {preserve_err}"),
                    json,
                ),
            }
            return Ok(errors::render_error(&e, json));
        }
    }

    if let Err(e) = stopped.delete() {
        render_warning("delete_failed", &format!("delete failed: {e}"), json);
    }

    Ok(guest_exit)
}

fn should_writeback(writeback: WritebackMode, guest_exit: i32) -> bool {
    match writeback {
        WritebackMode::Never => false,
        WritebackMode::OnSuccess => guest_exit == 0,
        WritebackMode::Always => true,
    }
}

fn network_policy_for_egress(egress: EgressMode) -> NetworkPolicy {
    match egress {
        EgressMode::None => NetworkPolicy::NoEgress,
        EgressMode::Outbound => NetworkPolicy::AllowOutbound { exceptions: vec![] },
    }
}

fn sandbox_config_for_run(
    workspace: Option<PathBuf>,
    egress: EgressMode,
    scratch_size: Option<u64>,
    request_id: String,
) -> SandboxConfig {
    SandboxConfig {
        vm_id: None,
        workspace,
        network: network_policy_for_egress(egress),
        vcpu_count: None,
        mem_size_mib: None,
        boot_args: None,
        overlay_size_bytes: scratch_size.unwrap_or(512 * 1024 * 1024),
        idle_timeout: None,
        request_id: Some(request_id),
    }
}

fn parse_env(values: Vec<String>) -> Result<Option<Vec<(String, String)>>, FcError> {
    let mut pairs = Vec::with_capacity(values.len());
    for value in values {
        let Some((key, val)) = value.split_once('=') else {
            return Err(FcError::Config(format!(
                "environment override must be KEY=VAL, got `{value}`"
            )));
        };
        if key.is_empty() {
            return Err(FcError::Config(
                "environment override key must not be empty".to_owned(),
            ));
        }
        pairs.push((key.to_owned(), val.to_owned()));
    }
    Ok((!pairs.is_empty()).then_some(pairs))
}

fn build_process_env(
    values: Vec<String>,
    secret_keys: Vec<String>,
) -> Result<Option<Vec<(String, String)>>, FcError> {
    let mut pairs = parse_env(values)?.unwrap_or_default();
    for key in secret_keys {
        validate_secret_env_key(&key)?;
        let value = std::env::var(&key)
            .map_err(|_| FcError::Config(format!("secret env `{key}` is not set")))?;
        pairs.push((key, value));
    }
    Ok((!pairs.is_empty()).then_some(pairs))
}

fn validate_secret_env_key(key: &str) -> Result<(), FcError> {
    if key.is_empty() {
        return Err(FcError::Config(
            "secret env key must not be empty".to_owned(),
        ));
    }
    if key.contains('=') {
        return Err(FcError::Config(format!(
            "secret env key must be a variable name, got `{key}`"
        )));
    }
    Ok(())
}

pub(super) fn render_not_implemented(message: &str, json: bool) -> i32 {
    if json {
        let request_id = crate::request_id::current();
        let obj = serde_json::json!({
            "request_id": request_id,
            "variant": "NotImplemented",
            "detail": message,
            "exit_code": errors::EXIT_NOT_IMPLEMENTED,
        });
        eprintln!("{}", json::to_pretty(&obj));
    } else if let Some(request_id) = crate::request_id::current() {
        eprintln!("error: [{request_id}] {message}");
    } else {
        eprintln!("error: {message}");
    }
    errors::EXIT_NOT_IMPLEMENTED
}

fn render_warning(code: &str, detail: &str, json: bool) {
    if json {
        let obj = serde_json::json!({
            "level": "warning",
            "code": code,
            "detail": detail,
        });
        eprintln!("{}", json::to_pretty(&obj));
    } else {
        eprintln!("warning: {detail}");
    }
}

/// `m80 preflight` — run host capability checks and render a table.
pub fn cmd_preflight(json: bool) -> anyhow::Result<i32> {
    Ok(render_preflight_result(m80_preflight::run(), json))
}

fn render_preflight_result(result: Result<Discovery, PreflightError>, json: bool) -> i32 {
    match result {
        Ok(discovery) => {
            if json {
                let rows = &discovery.report;
                println!("{}", format_preflight_json(rows));
            } else {
                println!("{}", discovery.render_table());
            }
            0
        }
        Err(e) => {
            let fc_err = m80_firecracker::FcError::Preflight(e);
            errors::render_error(&fc_err, json)
        }
    }
}

pub fn cmd_warm(action: WarmAction, json: bool) -> anyhow::Result<i32> {
    warm::cmd_warm(action, json)
}

pub fn cmd_quickstart(args: QuickstartArgs, json: bool) -> anyhow::Result<i32> {
    quickstart::cmd_quickstart(args, json)
}

pub fn cmd_env(json: bool) -> anyhow::Result<i32> {
    env::cmd_env(json)
}

fn emit_guest_boot_trace_if_enabled(run_dir: &Path) {
    if std::env::var("M80_PHASE_TRACE").ok().as_deref() != Some("1") {
        return;
    }
    let console = run_dir.join("console.log");
    let Ok(file) = File::open(&console) else {
        return;
    };
    for line in BufReader::new(file).lines().map_while(Result::ok) {
        if line.starts_with("M80_GUEST_BOOT ") {
            eprintln!("{line}");
        }
    }
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
        println!("{}", format_cleanup_json());
    } else {
        println!("cleanup complete");
    }

    Ok(0)
}

fn format_preflight_json(rows: &[CheckRow]) -> String {
    json::to_pretty(rows)
}

fn format_cleanup_json() -> String {
    let obj = serde_json::json!({ "status": "ok" });
    json::to_pretty(&obj)
}

/// `m80 config show` — reveal the merged effective config with field sources.
pub fn cmd_config_show(json: bool) -> anyhow::Result<i32> {
    let effective = match config::load_effective(&HashMap::new()) {
        Ok(result) => result,
        Err(e) => {
            return Ok(errors::render_error(
                &FcError::Config(format!("{e:#}")),
                json,
            ))
        }
    };

    if json {
        println!("{}", format_config_json(&effective));
    } else {
        print!("{}", format_config_table(&effective));
    }

    Ok(0)
}

fn format_config_table(effective: &EffectiveConfig) -> String {
    let mut out = String::new();
    out.push_str(&format!("{:<25} {:<20} SOURCE\n", "FIELD", "VALUE"));
    out.push_str(&format!("{}\n", "-".repeat(60)));
    for field in &effective.fields {
        out.push_str(&format!(
            "{:<25} {:<20} {:?}\n",
            field.name, field.value, field.source
        ));
    }
    out
}

fn format_config_json(effective: &EffectiveConfig) -> String {
    json::to_pretty(effective)
}

pub fn cmd_version(json: bool) -> anyhow::Result<i32> {
    version::cmd_version(json)
}

mod env;
mod pty;
mod quickstart;
mod run_request;
mod run_stream;
mod version;
mod warm;

#[cfg(test)]
mod tests;
