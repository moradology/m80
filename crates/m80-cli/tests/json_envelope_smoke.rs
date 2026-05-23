//! Script-facing smoke tests for the shared `m80 --json` envelope.

mod common;

use common::m80;

#[test]
fn json_output_envelope_stable_across_subcommands() {
    assert_success_json(["--json", "version"]);
    assert_success_json(["--json", "env"]);
    assert_success_json(["--json", "config", "show"]);
    assert_success_json(["--json", "warm", "disable"]);
    let run_root = synthetic_logs_run_root();
    assert_success_json_with_env(
        ["--json", "logs", "vm-a"],
        Some(("M80_RUN_ROOT", run_root.path().to_path_buf())),
    );
    assert_error_json(["--json", "run", "--scratch-size", "0", "--", "true"]);
    assert_preflight_json();
}

#[test]
fn env_json_on_failed_preflight_stable_schema() {
    let output = m80()
        .args(["--json", "env"])
        .env("M80_FIRECRACKER_BIN", "/definitely/missing/firecracker")
        .env("M80_JAIL_UID", "65534")
        .env("M80_JAIL_GID", "65534")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    assert!(
        output.stderr.is_empty(),
        "env JSON command must keep stderr empty: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|e| panic!("env output must be one JSON value: {e}\n{output:?}"));
    assert_eq!(parsed["version"], 1);
    assert_eq!(parsed["data"]["version"], 1);

    let preflight = &parsed["data"]["preflight"];
    assert_eq!(preflight["ok"], false);
    assert!(
        preflight
            .get("error")
            .and_then(serde_json::Value::as_str)
            .is_some(),
        "failed preflight must include an error string: {parsed}"
    );
    assert_eq!(
        preflight
            .get("checks")
            .and_then(serde_json::Value::as_array)
            .map(Vec::len),
        Some(0),
        "failed preflight must use an empty checks array: {parsed}"
    );
    assert_eq!(
        preflight["host_prerequisite_failure"]["remediation"]["id"], "repair-privilege",
        "env must expose the same remediation token as preflight JSON/text: {parsed}"
    );
    assert_eq!(
        preflight["host_prerequisite_failure"]["remediation"]["policy_link"],
        "docs/ops/host-setup.md"
    );
}

fn assert_success_json<const N: usize>(args: [&str; N]) {
    assert_success_json_with_env(args, None);
}

fn assert_success_json_with_env<const N: usize>(
    args: [&str; N],
    env: Option<(&'static str, std::path::PathBuf)>,
) {
    let mut cmd = m80();
    cmd.args(args);
    if let Some((key, value)) = env {
        cmd.env(key, value);
    }
    let output = cmd.output().unwrap();

    assert_eq!(output.status.code(), Some(0));
    assert!(
        output.stderr.is_empty(),
        "successful JSON command must keep stderr empty: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_json_envelope(&output.stdout, "stdout");
}

fn assert_error_json<const N: usize>(args: [&str; N]) {
    let output = m80().args(args).output().unwrap();

    assert_ne!(output.status.code(), Some(0));
    assert!(
        output.stdout.is_empty(),
        "error JSON command must keep stdout empty: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert_json_envelope(&output.stderr, "stderr");
}

fn assert_preflight_json() {
    let output = m80().args(["--json", "preflight"]).output().unwrap();
    let bytes = if output.status.success() {
        assert!(output.stderr.is_empty());
        &output.stdout
    } else {
        assert!(output.stdout.is_empty());
        &output.stderr
    };
    assert_json_envelope(bytes, "preflight output");
}

fn assert_json_envelope(bytes: &[u8], label: &str) {
    let parsed: serde_json::Value = serde_json::from_slice(bytes)
        .unwrap_or_else(|e| panic!("{label} must be one JSON value: {e}\n{bytes:?}"));
    assert_eq!(parsed["version"], 1, "{label} must use envelope version 1");
    assert!(
        parsed.get("data").is_some(),
        "{label} must carry a data field: {parsed}"
    );
    if let Some(request_id) = parsed.get("request_id") {
        let request_id = request_id
            .as_str()
            .unwrap_or_else(|| panic!("{label} request_id must be a string: {parsed}"));
        assert!(
            request_id.starts_with("req_"),
            "{label} request_id must be opaque m80 request id, got {request_id:?}"
        );
    }
}

fn synthetic_logs_run_root() -> tempfile::TempDir {
    let run_root = tempfile::tempdir().unwrap();
    let run_dir = run_root.path().join("vm-a");
    std::fs::create_dir_all(&run_dir).unwrap();
    std::fs::write(
        run_dir.join("diagnostics.jsonl"),
        r#"{"schema_version":2,"timestamp_unix_ms":2000,"event_kind":"lifecycle","source_class":"host","phase":"Request","message":"exec request started","request_id":"req-1","context":{"vm_id":"vm-a"}}
"#,
    )
    .unwrap();
    run_root
}
