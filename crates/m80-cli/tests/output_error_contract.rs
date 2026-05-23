//! Script-facing output and error contract tests.

mod common;

use common::m80;

const EXIT_CONFIG: i32 = 6;
const EXIT_GENERIC: i32 = 1;

#[test]
fn config_wrapper_failure_uses_stderr_exit_code_and_empty_stdout() {
    let output = m80()
        .args(["run", "--scratch-size", "0", "--", "true"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(EXIT_CONFIG));
    assert!(
        output.stdout.is_empty(),
        "wrapper failures must not write stdout metadata: {}",
        String::from_utf8_lossy(&output.stdout)
    );

    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("error: [req_"));
    assert!(stderr.contains("config:"));
    assert!(stderr.contains("--scratch-size must be greater than zero"));
}

#[test]
fn json_wrapper_failure_uses_stderr_envelope_and_empty_stdout() {
    let output = m80()
        .args(["--json", "run", "--scratch-size", "0", "--", "true"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(EXIT_CONFIG));
    assert!(
        output.stdout.is_empty(),
        "--json wrapper failures must keep stdout empty: {}",
        String::from_utf8_lossy(&output.stdout)
    );

    let stderr = String::from_utf8(output.stderr).unwrap();
    let parsed: serde_json::Value =
        serde_json::from_str(&stderr).expect("stderr should be one JSON envelope");
    assert_eq!(parsed["version"], 1);
    assert!(parsed["request_id"].as_str().unwrap().starts_with("req_"));
    assert!(
        parsed["data"].get("request_id").is_none(),
        "request_id must not be duplicated inside data"
    );
    assert_eq!(parsed["data"]["variant"], "Config");
    assert!(parsed["data"]["detail"]
        .as_str()
        .unwrap()
        .contains("--scratch-size must be greater than zero"));
}

#[test]
fn writeback_without_workspace_fails_before_backend_work() {
    let output = m80()
        .args(["run", "--writeback", "on-success", "--", "true"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(EXIT_CONFIG));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("error: [req_"));
    assert!(stderr.contains("config:"));
    assert!(stderr.contains("--writeback requires --workspace"));
}

#[test]
fn run_without_installed_profile_reports_repair_code_before_preflight() {
    let run_root = tempfile::tempdir().unwrap();
    let output = m80()
        .env("M80_DEFAULT_PROFILE", "env")
        .env_remove("M80_KERNEL_IMAGE")
        .env_remove("M80_ROOTFS_IMAGE")
        .env_remove("M80_ARTIFACT_DIR")
        .env("M80_RUN_ROOT", run_root.path())
        .args(["--json", "run", "--", "echo", "hello"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(EXIT_CONFIG));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    let parsed: serde_json::Value =
        serde_json::from_str(&stderr).expect("stderr should be one JSON envelope");
    assert_eq!(parsed["data"]["variant"], "Config");
    assert_eq!(parsed["data"]["code"], "no_installed_profile");
    let detail = parsed["data"]["detail"].as_str().unwrap();
    assert!(detail.contains("no installed default profile"), "{detail}");
    assert!(
        detail.contains("m80 install --bundle-url file:///path/to/m80-linux-x86_64.tar.gz"),
        "{detail}"
    );
    assert!(
        !detail.contains("releases/latest"),
        "dev build must not suggest latest install: {detail}"
    );
    assert!(
        common::run_root_entries(run_root.path()).is_empty(),
        "no-profile diagnostic must not create run dirs"
    );
}

#[test]
fn cli_exit_codes_parse_vs_runtime_distinct() {
    let parse_error = m80()
        .args(["run", "--not-a-real-flag", "--", "true"])
        .output()
        .unwrap();
    let runtime_config = m80()
        .args(["run", "--scratch-size", "0", "--", "true"])
        .output()
        .unwrap();
    let run_root = tempfile::tempdir().unwrap();
    let runtime_generic = m80()
        .env("M80_RUN_ROOT", run_root.path())
        .args(["run", "--warm", "--", "true"])
        .output()
        .unwrap();

    let parse_code = parse_error.status.code().expect("parse exit code");
    let config_code = runtime_config.status.code().expect("config exit code");
    let generic_code = runtime_generic.status.code().expect("generic exit code");

    assert_eq!(parse_code, 2, "clap parse errors should use EX_USAGE");
    assert_eq!(config_code, EXIT_CONFIG);
    assert_eq!(generic_code, EXIT_GENERIC);
}
