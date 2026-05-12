//! Runtime smoke tests for documented feature-gap stubs.
//!
//! These paths must fail before preflight/backend work, so they are safe on a
//! machine without KVM or Firecracker artifacts.

mod common;

use common::m80;

const EXIT_CONFIG: i32 = 6;
const EXIT_GENERIC: i32 = 1;
const EXIT_NOT_IMPLEMENTED: i32 = 7;

fn assert_feature_gap(args: &[&str], expected_stderr: &str) {
    let output = m80().args(args).output().unwrap();
    assert_eq!(output.status.code(), Some(EXIT_NOT_IMPLEMENTED));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains(expected_stderr),
        "stderr should explain feature gap `{expected_stderr}`, got: {stderr}"
    );
}

#[test]
fn warm_system_enable_is_explicit_feature_gap() {
    assert_feature_gap(
        &["warm", "enable", "--system", "--size", "1"],
        "`m80 warm enable --system` is reserved",
    );
}

#[test]
fn warm_system_feature_gap_honors_json_mode() {
    let output = m80()
        .args(["--json", "warm", "enable", "--system", "--size", "1"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(EXIT_NOT_IMPLEMENTED));
    let stderr = String::from_utf8(output.stderr).unwrap();
    let parsed: serde_json::Value =
        serde_json::from_str(&stderr).expect("feature-gap JSON should parse");
    assert_eq!(parsed["version"], 1);
    assert_eq!(parsed["data"]["variant"], "NotImplemented");
    assert_eq!(parsed["data"]["exit_code"], EXIT_NOT_IMPLEMENTED);
}

#[test]
fn warm_status_without_owner_is_unavailable_without_preflight() {
    let output = m80().args(["warm", "status"]).output().unwrap();

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("owner: unavailable"));
    assert!(stdout.contains("owner_unavailable"));
}

#[test]
fn warm_disable_without_owner_is_disabled_without_preflight() {
    let output = m80().args(["--json", "warm", "disable"]).output().unwrap();

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8(output.stdout).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(parsed["version"], 1);
    assert_eq!(parsed["data"]["owner"]["state"], "disabled");
}

#[test]
fn run_warm_without_owner_fails_without_cold_booting() {
    let run_root = tempfile::tempdir().unwrap();
    let output = m80()
        .env("M80_RUN_ROOT", run_root.path())
        .args(["run", "--warm", "--", "true"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(EXIT_GENERIC));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("warm owner unavailable"));
    assert!(
        std::fs::read_dir(run_root.path()).unwrap().next().is_none(),
        "warm run without owner must not create cold-boot run dirs"
    );
}

#[test]
fn run_warm_workspace_fails_before_owner_lookup() {
    let run_root = tempfile::tempdir().unwrap();
    let workspace = tempfile::tempdir().unwrap();
    let output = m80()
        .env("M80_RUN_ROOT", run_root.path())
        .args([
            "run",
            "--warm",
            "--workspace",
            workspace.path().to_str().unwrap(),
            "--",
            "true",
        ])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(EXIT_CONFIG));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("--warm --workspace is unsupported"));
    assert!(
        std::fs::read_dir(run_root.path()).unwrap().next().is_none(),
        "warm workspace rejection must happen before owner/cold work"
    );
}

#[test]
fn run_egress_allowlist_is_explicit_feature_gap() {
    assert_feature_gap(
        &["run", "--allow-host", "api.openai.com", "--", "true"],
        "reserved for egress allowlists",
    );
}

#[test]
fn run_mount_config_is_explicit_feature_gap() {
    assert_feature_gap(
        &[
            "run",
            "--mount-config",
            "/home/me/.config/tool:/config/tool:ro",
            "--",
            "true",
        ],
        "reserved for explicit config-file projection",
    );
}

#[test]
fn run_keep_on_failure_is_explicit_feature_gap() {
    assert_feature_gap(
        &["run", "--keep-on-failure", "--", "true"],
        "reserved for diagnostics retention",
    );
}

#[test]
fn run_tty_json_is_config_error_before_preflight() {
    let output = m80()
        .args(["--json", "run", "--tty", "--", "bash"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(EXIT_CONFIG));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&stderr).unwrap();
    assert_eq!(parsed["version"], 1);
    assert!(parsed["request_id"].as_str().unwrap().starts_with("req_"));
    assert_eq!(parsed["data"]["request_id"], parsed["request_id"]);
    assert_eq!(parsed["data"]["variant"], "Config");
    assert_eq!(parsed["data"]["exit_code"], EXIT_CONFIG);
    assert!(parsed["data"]["detail"]
        .as_str()
        .unwrap()
        .contains("--tty is incompatible with --json"));
}

#[test]
fn run_interactive_without_tty_is_config_error_before_preflight() {
    let output = m80().args(["run", "-i", "--", "bash"]).output().unwrap();

    assert_eq!(output.status.code(), Some(EXIT_CONFIG));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("error: [req_"));
    assert!(stderr.contains("config:"));
    assert!(stderr.contains("-i requires --tty / -t"));
}

#[test]
fn run_stdin_with_tty_is_config_error_before_preflight() {
    let output = m80()
        .args(["run", "--stdin", "--tty", "--", "bash"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(EXIT_CONFIG));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("error: [req_"));
    assert!(stderr.contains("config:"));
    assert!(stderr.contains("--stdin is incompatible with --tty"));
}
