//! Script-facing output and error contract tests.

use assert_cmd::Command;
use m80_cli::errors::{
    envelope, exit_code_for, EXIT_ADMISSION, EXIT_CONFIG, EXIT_GENERIC, EXIT_INVALID_STATE,
    EXIT_MANIFEST, EXIT_NOT_IMPLEMENTED, EXIT_POOL_EMPTY, EXIT_PREFLIGHT,
};
use m80_firecracker::FcError;
use m80_image_manifest::ManifestError;
use m80_preflight::PreflightError;

fn m80() -> Command {
    Command::cargo_bin("m80").unwrap()
}

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
fn documented_fc_error_classes_have_cli_exit_codes_and_payloads() {
    let cases = [
        (
            FcError::Io(std::io::Error::new(std::io::ErrorKind::Other, "io")),
            EXIT_GENERIC,
            "Io",
        ),
        (
            FcError::Preflight(PreflightError::KvmUnavailable {
                path: "/dev/kvm".into(),
            }),
            EXIT_PREFLIGHT,
            "Preflight",
        ),
        (
            FcError::AdmissionRefused { limit: 8 },
            EXIT_ADMISSION,
            "AdmissionRefused",
        ),
        (
            FcError::Manifest(ManifestError::UnsupportedSchemaVersion(0)),
            EXIT_MANIFEST,
            "Manifest",
        ),
        (
            FcError::InvalidState {
                expected: "Running",
                actual: "Stopped",
            },
            EXIT_INVALID_STATE,
            "InvalidState",
        ),
        (
            FcError::Config("bad config".to_owned()),
            EXIT_CONFIG,
            "Config",
        ),
        (
            FcError::PoolEmpty { target_ready: 1 },
            EXIT_POOL_EMPTY,
            "PoolEmpty",
        ),
        (
            FcError::ApiSocketTimeout {
                path: "/run/m80/firecracker.sock".into(),
                timeout: std::time::Duration::from_secs(5),
            },
            EXIT_GENERIC,
            "ApiSocketTimeout",
        ),
        (
            FcError::GuestdReadyTimeout {
                path: "/run/m80/vsock.sock_9000".into(),
                timeout: std::time::Duration::from_secs(60),
            },
            EXIT_GENERIC,
            "GuestdReadyTimeout",
        ),
    ];

    for (err, expected_code, expected_variant) in cases {
        assert_eq!(exit_code_for(&err), expected_code, "{err:?}");
        let payload = envelope(&err);
        assert_eq!(payload.variant, expected_variant, "{err:?}");
        assert_eq!(payload.exit_code, expected_code, "{err:?}");
        assert!(!payload.detail.is_empty(), "{err:?}");
    }
}
