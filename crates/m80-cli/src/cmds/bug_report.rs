use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use m80_firecracker::{ConfigError, FcError, CONSOLE_LOG};
use m80_observability::DIAGNOSTICS_FILE_NAME;
use serde::Serialize;
use serde_json::{json, Value};

use crate::args::BugReportArgs;
use crate::config;
use crate::errors;
use crate::install_state::{
    resolve_install_state, InstallStatePaths, InstallStateReport, InstallStateRequest,
};
use crate::json as cli_json;

use super::{env, install_status};

const BUG_REPORT_SCHEMA_VERSION: u16 = 1;
const MAX_LOG_TAIL_LINES: usize = 1_000;

pub(super) fn cmd_bug_report(args: BugReportArgs, _json_mode: bool) -> anyhow::Result<i32> {
    let tail_lines = match validate_tail_lines(args.log_tail_lines) {
        Ok(tail_lines) => tail_lines,
        Err(e) => return Ok(errors::render_error(&e, true)),
    };
    let report = match collect_bug_report(args, tail_lines) {
        Ok(report) => report,
        Err(e) => return Ok(errors::render_error(&e, true)),
    };
    println!("{}", cli_json::to_pretty(&report));
    Ok(0)
}

fn validate_tail_lines(value: usize) -> Result<usize, FcError> {
    if value > MAX_LOG_TAIL_LINES {
        return Err(FcError::Config(ConfigError::InvalidValue {
            field: "log_tail_lines",
            reason: format!("must be <= {MAX_LOG_TAIL_LINES}, got {value}"),
        }));
    }
    Ok(value)
}

fn collect_bug_report(args: BugReportArgs, tail_lines: usize) -> Result<Value, FcError> {
    let install_report = resolve_install_state(InstallStateRequest {
        paths: InstallStatePaths::host(args.install_root),
        profile_override: args.profile,
    });
    let install_status = serde_json::to_value(install_status::InstallStatusOutput::from_report(
        &install_report,
    ))
    .unwrap();
    let env_dump = env::collect_env_dump_value();
    let run_root = args
        .run_root
        .map(Ok)
        .unwrap_or_else(config::resolve_run_root);
    let logs = collect_logs(
        args.vm_id.as_deref(),
        args.request_id.as_deref(),
        run_root,
        tail_lines,
    )?;

    let raw = BugReportBundle {
        schema_version: BUG_REPORT_SCHEMA_VERSION,
        m80_version: crate::release::DISPLAY_VERSION,
        selected_release: SelectedRelease::from_install_report(&install_report),
        install_status,
        verifier_diagnostics: verifier_diagnostics(&install_report),
        host_prerequisite_summary: host_prerequisite_summary(&env_dump),
        logs,
    };
    let mut value = serde_json::to_value(raw).unwrap();
    let redaction = redact_bundle(&mut value);
    value.as_object_mut().unwrap().insert(
        "redaction".to_owned(),
        serde_json::to_value(redaction).unwrap(),
    );
    Ok(value)
}

#[derive(Serialize)]
struct BugReportBundle {
    schema_version: u16,
    m80_version: &'static str,
    selected_release: SelectedRelease,
    install_status: Value,
    verifier_diagnostics: Value,
    host_prerequisite_summary: Value,
    logs: LogBundle,
}

#[derive(Serialize)]
struct SelectedRelease {
    active_release_tag: Option<String>,
    selected_profile_release_tag: Option<String>,
    selected_profile_m80_version: Option<String>,
}

impl SelectedRelease {
    fn from_install_report(report: &InstallStateReport) -> Self {
        Self {
            active_release_tag: report.active_pointer.release_tag.clone(),
            selected_profile_release_tag: report
                .profile
                .as_ref()
                .and_then(|profile| profile.release_tag.clone()),
            selected_profile_m80_version: report
                .profile
                .as_ref()
                .and_then(|profile| profile.m80_version.clone()),
        }
    }
}

fn verifier_diagnostics(report: &InstallStateReport) -> Value {
    let proof_cache = install_status::ProofCacheStatusOutput::from_install_report(report);
    json!({
        "install_state": report.state,
        "diagnostics": report.diagnostics,
        "proof_cache": proof_cache,
    })
}

fn host_prerequisite_summary(env_dump: &Value) -> Value {
    json!({
        "host": env_dump.get("host").cloned().unwrap_or(Value::Null),
        "preflight": env_dump.get("preflight").cloned().unwrap_or(Value::Null),
    })
}

#[derive(Serialize)]
struct LogBundle {
    vm_id: Option<String>,
    request_id: Option<String>,
    run_root: Option<PathBuf>,
    status: LogBundleStatus,
    tail_lines_per_file: usize,
    files: Vec<LogTail>,
    error: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum LogBundleStatus {
    NotRequested,
    RunRootUnavailable,
    RunDirMissing,
    Collected,
}

#[derive(Serialize)]
struct LogTail {
    name: &'static str,
    path: PathBuf,
    line_count: usize,
    omitted_line_count: usize,
    truncated: bool,
    lines: Vec<String>,
}

fn collect_logs(
    vm_id: Option<&str>,
    request_id: Option<&str>,
    run_root: Result<PathBuf, FcError>,
    tail_lines: usize,
) -> Result<LogBundle, FcError> {
    let Some(vm_id) = vm_id else {
        return Ok(LogBundle {
            vm_id: None,
            request_id: request_id.map(str::to_owned),
            run_root: run_root.ok(),
            status: LogBundleStatus::NotRequested,
            tail_lines_per_file: tail_lines,
            files: Vec::new(),
            error: None,
        });
    };
    let run_root = match run_root {
        Ok(run_root) => run_root,
        Err(e) => {
            return Ok(LogBundle {
                vm_id: Some(vm_id.to_owned()),
                request_id: request_id.map(str::to_owned),
                run_root: None,
                status: LogBundleStatus::RunRootUnavailable,
                tail_lines_per_file: tail_lines,
                files: Vec::new(),
                error: Some(e.to_string()),
            });
        }
    };
    let run_dir = run_root.join(vm_id);
    if !run_dir.exists() {
        return Ok(LogBundle {
            vm_id: Some(vm_id.to_owned()),
            request_id: request_id.map(str::to_owned),
            run_root: Some(run_root),
            status: LogBundleStatus::RunDirMissing,
            tail_lines_per_file: tail_lines,
            files: Vec::new(),
            error: Some(format!("run directory not found: {}", run_dir.display())),
        });
    }

    let mut files = Vec::new();
    files.push(read_log_tail(
        "host_diagnostics",
        &run_dir.join(DIAGNOSTICS_FILE_NAME),
        request_id,
        tail_lines,
    )?);
    files.push(read_log_tail(
        "guest_console",
        &run_dir.join(CONSOLE_LOG),
        request_id,
        tail_lines,
    )?);
    Ok(LogBundle {
        vm_id: Some(vm_id.to_owned()),
        request_id: request_id.map(str::to_owned),
        run_root: Some(run_root),
        status: LogBundleStatus::Collected,
        tail_lines_per_file: tail_lines,
        files,
        error: None,
    })
}

fn read_log_tail(
    name: &'static str,
    path: &Path,
    request_id: Option<&str>,
    tail_lines: usize,
) -> Result<LogTail, FcError> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(source) => {
            return Err(FcError::PathIo {
                path: path.to_path_buf(),
                source,
            });
        }
    };
    let mut lines = text
        .lines()
        .filter(|line| request_id.is_none_or(|id| line.contains(id)))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let line_count = lines.len();
    let omitted_line_count = line_count.saturating_sub(tail_lines);
    if omitted_line_count > 0 {
        lines = lines.split_off(omitted_line_count);
    }
    Ok(LogTail {
        name,
        path: path.to_path_buf(),
        line_count,
        omitted_line_count,
        truncated: omitted_line_count > 0,
        lines,
    })
}

#[derive(Serialize)]
struct RedactionReport {
    status: RedactionStatus,
    replacements: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum RedactionStatus {
    Passed,
}

fn redact_bundle(value: &mut Value) -> RedactionReport {
    let rules = RedactionRules::from_host_env();
    let mut replacements = BTreeSet::new();
    redact_value(value, &rules, &mut replacements);
    RedactionReport {
        status: RedactionStatus::Passed,
        replacements: replacements.into_iter().collect(),
    }
}

struct RedactionRules {
    paths: Vec<(String, &'static str)>,
}

impl RedactionRules {
    fn from_host_env() -> Self {
        let mut paths = Vec::new();
        push_env_path(&mut paths, "HOME", "<redacted:home>");
        push_env_path(&mut paths, "RUNNER_TEMP", "<redacted:runner_temp>");
        push_env_path(
            &mut paths,
            "GITHUB_WORKSPACE",
            "<redacted:github_workspace>",
        );
        if let Ok(path) = std::env::current_dir() {
            push_path(&mut paths, path, "<redacted:workspace>");
        }
        paths.sort_by(|a, b| b.0.len().cmp(&a.0.len()));
        Self { paths }
    }
}

fn push_env_path(paths: &mut Vec<(String, &'static str)>, key: &str, replacement: &'static str) {
    if let Some(path) = std::env::var_os(key).map(PathBuf::from) {
        push_path(paths, path, replacement);
    }
}

fn push_path(paths: &mut Vec<(String, &'static str)>, path: PathBuf, replacement: &'static str) {
    if path.as_os_str().is_empty() {
        return;
    }
    let value = path.to_string_lossy().to_string();
    if value == "/" || value.len() < 4 || paths.iter().any(|(seen, _)| seen == &value) {
        return;
    }
    paths.push((value, replacement));
}

fn redact_value(value: &mut Value, rules: &RedactionRules, replacements: &mut BTreeSet<String>) {
    match value {
        Value::String(text) => {
            let redacted = redact_string(text, rules, replacements);
            *text = redacted;
        }
        Value::Array(items) => {
            for item in items {
                redact_value(item, rules, replacements);
            }
        }
        Value::Object(map) => {
            for item in map.values_mut() {
                redact_value(item, rules, replacements);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

fn redact_string(
    input: &str,
    rules: &RedactionRules,
    replacements: &mut BTreeSet<String>,
) -> String {
    let mut text = input.to_owned();
    if text.contains("-----BEGIN ") && text.contains(" PRIVATE KEY-----") {
        text = "<redacted:ssh_private_key>".to_owned();
        replacements.insert("ssh_private_key".to_owned());
    }
    for prefix in ["github_pat_", "ghp_", "gho_", "ghu_", "ghs_", "ghr_"] {
        text = redact_prefixed_token(&text, prefix, "github_credential", replacements);
    }
    for (path, replacement) in &rules.paths {
        if text.contains(path) {
            text = text.replace(path, replacement);
            replacements.insert(replacement.trim_matches(&['<', '>'][..]).to_owned());
        }
    }
    text
}

fn redact_prefixed_token(
    input: &str,
    prefix: &str,
    label: &str,
    replacements: &mut BTreeSet<String>,
) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(index) = rest.find(prefix) {
        out.push_str(&rest[..index]);
        let token = &rest[index..];
        let end = token
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
            .unwrap_or(token.len());
        out.push_str("<redacted:github_credential>");
        replacements.insert(label.to_owned());
        rest = &token[end..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redaction_removes_secrets_and_local_paths() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        let runner = temp.path().join("runner");
        let workspace = temp.path().join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let _restore = m80_test_helpers::env::EnvRestore::capture(&[
            "HOME",
            "RUNNER_TEMP",
            "GITHUB_WORKSPACE",
        ]);
        std::env::set_var("HOME", &home);
        std::env::set_var("RUNNER_TEMP", &runner);
        std::env::set_var("GITHUB_WORKSPACE", &workspace);

        let mut value = json!({
            "token": "ghp_abcdefghijklmnopqrstuvwxyz",
            "key": "-----BEGIN OPENSSH PRIVATE KEY-----\nabc\n-----END OPENSSH PRIVATE KEY-----",
            "home_path": home.join("m80").display().to_string(),
            "runner_path": runner.join("tmp").display().to_string(),
            "workspace_path": workspace.join("repo").display().to_string(),
        });

        let report = redact_bundle(&mut value);
        let rendered = serde_json::to_string(&value).unwrap();
        assert!(!rendered.contains("ghp_abcdefghijklmnopqrstuvwxyz"));
        assert!(!rendered.contains("PRIVATE KEY"));
        assert!(!rendered.contains(&home.display().to_string()));
        assert!(!rendered.contains(&runner.display().to_string()));
        assert!(!rendered.contains(&workspace.display().to_string()));
        assert!(report
            .replacements
            .contains(&"github_credential".to_owned()));
        assert!(report.replacements.contains(&"ssh_private_key".to_owned()));
    }

    #[test]
    fn log_tail_is_bounded_and_request_filtered() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("diagnostics.jsonl");
        std::fs::write(
            &path,
            "req-a one\nreq-b two\nreq-a three\nreq-a four\nreq-b five\n",
        )
        .unwrap();

        let tail = read_log_tail("host_diagnostics", &path, Some("req-a"), 2).unwrap();

        assert_eq!(tail.line_count, 3);
        assert_eq!(tail.omitted_line_count, 1);
        assert!(tail.truncated);
        assert_eq!(tail.lines, vec!["req-a three", "req-a four"]);
    }

    #[test]
    fn tail_line_limit_fails_closed() {
        let err = validate_tail_lines(MAX_LOG_TAIL_LINES + 1).unwrap_err();
        assert!(err.to_string().contains("log_tail_lines"));
    }
}
