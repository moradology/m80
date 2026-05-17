use m80_firecracker::{FcError, WireProtocolError};
use m80_proto::ExecRequest;

use super::common::RunDirDumpGuard;

#[test]
#[ignore = "requires KVM host and M80_MALICIOUS_ARTIFACT_DIR image built with m80-guestd-malicious"]
fn bogus_request_id_returns_request_id_mismatch_with_expected_and_observed_ids() {
    let (_backend, mut running, run_dir) = super::launch_malicious("bogus_request_id");
    let _dump_guard = RunDirDumpGuard::new(run_dir.clone());

    let err = running
        .exec(ExecRequest {
            program: "/bin/true".into(),
            args: Vec::new(),
            cwd: None,
            env: None,
            stdin: None,
            timeout_ms: Some(5_000),
            streaming: false,
        })
        .expect_err("bogus response request id must fail");

    assert!(
        matches!(
            err,
            FcError::Protocol(WireProtocolError::RequestIdMismatch {
                context: "exec exit",
                ref expected,
                got: Some(ref got),
            }) if expected.starts_with("malicious-bogus_request_id")
                && expected.contains("-exec-")
                && got == "malicious-stale-request-id"
        ),
        "expected request-id mismatch, got {err:?}"
    );
    let diagnostics_path = run_dir.join("diagnostics.jsonl");
    let diagnostics = std::fs::read_to_string(&diagnostics_path)
        .unwrap_or_else(|e| panic!("read {}: {e}", diagnostics_path.display()));
    for needle in ["request_id mismatch in exec exit", "malicious-stale-request-id"] {
        assert!(
            diagnostics.contains(needle),
            "diagnostics did not contain {needle:?}:\n{diagnostics}"
        );
    }

    running
        .force_kill()
        .expect("force kill bogus-request-id malicious VM")
        .delete()
        .expect("delete bogus-request-id malicious VM");
}
