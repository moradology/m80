use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufRead as _, BufReader, Read as _};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use m80_firecracker::{
    Backend, ConfigError, EffectiveConfig, FcError, NetworkPolicy, SandboxConfig, StoppedSandbox,
};
use m80_preflight::{Discovery, PreflightError};

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
    let effective = config::load_effective(flag_overrides)
        .map_err(|e| FcError::Config(ConfigError::Other(format!("{e:#}"))))?;
    backend_from_effective(effective)
}

fn build_run_backend(profile: Option<String>) -> Result<(Arc<Backend>, EffectiveConfig), FcError> {
    let mut flag_overrides = HashMap::new();
    if let Some(profile) = profile {
        flag_overrides.insert("default_profile", profile);
    }
    let effective = config::load_effective(&flag_overrides)
        .map_err(|e| FcError::Config(ConfigError::Other(format!("{e:#}"))))?;
    let runtime_profile = profile::resolve_from_effective(&effective, ProfileFilePaths::host())?;
    let _profile_env = runtime_profile.apply_env();
    backend_from_effective(effective)
}

fn backend_from_effective(
    effective: EffectiveConfig,
) -> Result<(Arc<Backend>, EffectiveConfig), FcError> {
    let run_root = effective
        .fields
        .iter()
        .find(|f| f.name == "run_root")
        .map(|f| std::path::PathBuf::from(&f.value));
    let artifact_config = match run_root {
        Some(run_root) => m80_preflight::ArtifactPreflightConfig {
            run_root,
            ..m80_preflight::ArtifactPreflightConfig::from_env()
        },
        None => m80_preflight::ArtifactPreflightConfig::from_env(),
    };
    let discovery = m80_preflight::run_with_configs(
        m80_preflight::BinaryDiscoveryConfig::from_env(),
        artifact_config,
    )?;
    let backend_config = config::backend_config(discovery, &effective)
        .map_err(|e| FcError::Config(ConfigError::Other(format!("{e:#}"))))?;
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

    if let Err(e) = validate_run_flags(interactive, tty, stdin, warm, workspace.is_some(), json) {
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
        let e = FcError::Config(ConfigError::MissingField { field: "argv" });
        return Ok(errors::render_error(&e, json));
    };
    if scratch_size == Some(0) {
        let e = FcError::Config(ConfigError::InvalidValue {
            field: "scratch_size",
            reason: "--scratch-size must be greater than zero".into(),
        });
        return Ok(errors::render_error(&e, json));
    }
    if writeback != WritebackMode::Never && workspace.is_none() {
        let e = FcError::Config(ConfigError::InvalidValue {
            field: "writeback",
            reason: "--writeback requires --workspace".into(),
        });
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
        println!(
            "{}",
            json::to_pretty(&proto_json::ExecResponseJson::from(response))
        );
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
        let workspace = workspace_for_writeback.as_ref().unwrap();
        if let Err(e) = extract_workspace_replacing(&stopped, workspace) {
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

fn extract_workspace_replacing(stopped: &StoppedSandbox, workspace: &Path) -> Result<(), FcError> {
    let backup = workspace.exists().then(|| writeback_backup_path(workspace));

    if let Some(backup) = &backup {
        fs::rename(workspace, backup)?;
    }

    match stopped.extract_changes(workspace) {
        Ok(_) => {
            if let Some(backup) = backup {
                remove_path(&backup)?;
            }
            Ok(())
        }
        Err(e) => {
            if let Some(backup) = backup {
                if workspace.exists() {
                    remove_path(workspace)?;
                }
                fs::rename(backup, workspace)?;
            }
            Err(e)
        }
    }
}

fn writeback_backup_path(workspace: &Path) -> PathBuf {
    let parent = workspace
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let name = workspace
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("workspace");
    parent.join(format!(
        ".{name}.m80-writeback-backup-{}-{}",
        std::process::id(),
        ulid::Ulid::new()
    ))
}

fn remove_path(path: &Path) -> Result<(), std::io::Error> {
    let meta = fs::symlink_metadata(path)?;
    if meta.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

fn validate_run_flags(
    interactive: bool,
    tty: bool,
    stdin: bool,
    warm: bool,
    has_workspace: bool,
    json: bool,
) -> Result<(), FcError> {
    if interactive && !tty {
        return Err(FcError::Config(ConfigError::InvalidValue {
            field: "interactive",
            reason: "-i requires --tty / -t".into(),
        }));
    }
    if tty && json {
        return Err(FcError::Config(ConfigError::InvalidValue {
            field: "tty",
            reason: "--tty is incompatible with --json".into(),
        }));
    }
    if tty && stdin {
        return Err(FcError::Config(ConfigError::InvalidValue {
            field: "stdin",
            reason: "--stdin is incompatible with --tty".into(),
        }));
    }
    if warm && has_workspace {
        return Err(FcError::Config(ConfigError::InvalidValue {
            field: "workspace",
            reason: "--warm --workspace is unsupported until attach-late workspace lands".into(),
        }));
    }
    if warm && tty {
        return Err(FcError::Config(ConfigError::InvalidValue {
            field: "tty",
            reason: "--warm is incompatible with --tty until warm terminal leases land".into(),
        }));
    }
    Ok(())
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
        daemonize: false,
        request_id: Some(request_id),
        preallocated_drive_slots: 0,
        one_shot: false,
    }
}

fn parse_env(values: Vec<String>) -> Result<Option<Vec<(String, String)>>, FcError> {
    let mut pairs = Vec::with_capacity(values.len());
    for value in values {
        let Some((key, val)) = value.split_once('=') else {
            return Err(FcError::Config(ConfigError::InvalidValue {
                field: "env",
                reason: format!("environment override must be KEY=VAL, got `{value}`"),
            }));
        };
        if key.is_empty() {
            return Err(FcError::Config(ConfigError::InvalidValue {
                field: "env",
                reason: "environment override key must not be empty".into(),
            }));
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
        let value = std::env::var(&key).map_err(|_| {
            FcError::Config(ConfigError::InvalidValue {
                field: "secret_env",
                reason: format!("secret env `{key}` is not set"),
            })
        })?;
        pairs.push((key, value));
    }
    Ok((!pairs.is_empty()).then_some(pairs))
}

fn validate_secret_env_key(key: &str) -> Result<(), FcError> {
    if key.is_empty() {
        return Err(FcError::Config(ConfigError::InvalidValue {
            field: "secret_env",
            reason: "secret env key must not be empty".into(),
        }));
    }
    if key.contains('=') {
        return Err(FcError::Config(ConfigError::InvalidValue {
            field: "secret_env",
            reason: format!("secret env key must be a variable name, got `{key}`"),
        }));
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
    let result = preflight_with_effective_config();
    Ok(render_preflight_result(result, json))
}

fn preflight_with_effective_config(
) -> Result<m80_preflight::Discovery, m80_preflight::PreflightError> {
    let run_root = config::load_effective(&std::collections::HashMap::new())
        .ok()
        .and_then(|eff| {
            eff.fields
                .into_iter()
                .find(|f| f.name == "run_root")
                .map(|f| std::path::PathBuf::from(f.value))
        });
    let artifact_config = match run_root {
        Some(run_root) => m80_preflight::ArtifactPreflightConfig {
            run_root,
            ..m80_preflight::ArtifactPreflightConfig::from_env()
        },
        None => m80_preflight::ArtifactPreflightConfig::from_env(),
    };
    m80_preflight::run_with_configs(
        m80_preflight::BinaryDiscoveryConfig::from_env(),
        artifact_config,
    )
}

fn render_preflight_result(result: Result<Discovery, PreflightError>, json: bool) -> i32 {
    match result {
        Ok(discovery) => {
            if json {
                let rows = &discovery.report;
                println!("{}", json::to_pretty(rows));
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
    if !std::env::var("M80_PHASE_TRACE").is_ok_and(|v| v == "1") {
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
        println!(
            "{}",
            json::to_pretty(&serde_json::json!({ "status": "ok" }))
        );
    } else {
        println!("cleanup complete");
    }

    Ok(0)
}

/// `m80 config show` — reveal the merged effective config with field sources.
pub fn cmd_config_show(json: bool) -> anyhow::Result<i32> {
    let effective = match config::load_effective(&HashMap::new()) {
        Ok(result) => result,
        Err(e) => {
            return Ok(errors::render_error(
                &FcError::Config(ConfigError::Other(format!("{e:#}"))),
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
mod proto_json;
mod pty;
mod quickstart;
mod run_request;
mod run_stream;
mod version;
mod warm;

#[cfg(test)]
mod tests;
