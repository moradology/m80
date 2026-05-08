//! Script-facing output and error contract tests.

mod common;

use common::m80;

use m80_cli::errors::{
    envelope, exit_code_for, EXIT_ADMISSION, EXIT_CONFIG, EXIT_GENERIC, EXIT_INVALID_STATE,
    EXIT_MANIFEST, EXIT_NOT_IMPLEMENTED, EXIT_POOL_EMPTY, EXIT_PREFLIGHT,
};
use m80_firecracker::{ConfigError, FcError};
use m80_image_manifest::ManifestError;
use m80_preflight::PreflightError;

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
    assert_eq!(parsed["data"]["request_id"], parsed["request_id"]);
    assert_eq!(parsed["data"]["variant"], "Config");
    assert_eq!(parsed["data"]["exit_code"], EXIT_CONFIG);
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
fn feature_gap_failure_uses_distinct_exit_code() {
    let output = m80()
        .args(["run", "--allow-host", "api.openai.com", "--", "bash"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(EXIT_NOT_IMPLEMENTED));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("reserved for egress allowlists"));
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
    assert_ne!(parse_code, config_code);
    assert_ne!(parse_code, generic_code);
    assert_ne!(config_code, generic_code);
}

fn assert_exit_code_and_payload(err: &FcError, expected_code: i32, expected_variant: &str) {
    assert_eq!(exit_code_for(err), expected_code, "{err:?}");
    let payload = envelope(err);
    assert_eq!(payload.variant, expected_variant, "{err:?}");
    assert_eq!(payload.exit_code, expected_code, "{err:?}");
    assert!(!payload.detail.is_empty(), "{err:?}");
}

#[test]
fn io_error_has_exit_code() {
    let err = FcError::Io(std::io::Error::new(std::io::ErrorKind::Other, "io"));
    assert_exit_code_and_payload(&err, EXIT_GENERIC, "Io");
}

#[test]
fn preflight_error_has_exit_code() {
    let err = FcError::Preflight(PreflightError::KvmUnavailable {
        path: "/dev/kvm".into(),
    });
    assert_exit_code_and_payload(&err, EXIT_PREFLIGHT, "Preflight");
}

#[test]
fn admission_refused_has_exit_code() {
    let err = FcError::AdmissionRefused { limit: 8 };
    assert_exit_code_and_payload(&err, EXIT_ADMISSION, "AdmissionRefused");
}

#[test]
fn manifest_error_has_exit_code() {
    let err = FcError::Manifest(ManifestError::UnsupportedSchemaVersion(0));
    assert_exit_code_and_payload(&err, EXIT_MANIFEST, "Manifest");
}

#[test]
fn invalid_state_has_exit_code() {
    let err = FcError::InvalidState {
        expected: "Running",
        actual: "Stopped",
    };
    assert_exit_code_and_payload(&err, EXIT_INVALID_STATE, "InvalidState");
}

#[test]
fn config_error_has_exit_code() {
    let err = FcError::Config(ConfigError::Other("bad config".to_owned()));
    assert_exit_code_and_payload(&err, EXIT_CONFIG, "Config");
}

#[test]
fn pool_empty_has_exit_code() {
    let err = FcError::PoolEmpty { target_ready: 1 };
    assert_exit_code_and_payload(&err, EXIT_POOL_EMPTY, "PoolEmpty");
}

#[test]
fn api_socket_timeout_has_exit_code() {
    let err = FcError::ApiSocketTimeout {
        path: "/run/m80/firecracker.sock".into(),
        timeout: std::time::Duration::from_secs(5),
    };
    assert_exit_code_and_payload(&err, EXIT_GENERIC, "ApiSocketTimeout");
}

#[test]
fn guestd_ready_timeout_has_exit_code() {
    let err = FcError::GuestdReadyTimeout {
        path: "/run/m80/vsock.sock_9000".into(),
        timeout: std::time::Duration::from_secs(60),
    };
    assert_exit_code_and_payload(&err, EXIT_GENERIC, "GuestdReadyTimeout");
}
