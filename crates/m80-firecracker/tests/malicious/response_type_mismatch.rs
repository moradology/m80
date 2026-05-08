use m80_firecracker::{FcError, WireProtocolError};
use m80_proto::ExecRequest;

use super::common::RunDirDumpGuard;

#[test]
#[ignore = "requires KVM host and M80_MALICIOUS_ARTIFACT_DIR image built with m80-guestd-malicious"]
fn response_type_mismatch_returns_malformed_peer_with_expected_and_observed_kinds() {
    let (_backend, mut running, run_dir) = super::launch_malicious("response_type_mismatch");
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
        .expect_err("wrong-shaped response must fail");

    assert!(
        matches!(
            err,
            FcError::Protocol(WireProtocolError::MalformedPeer(ref detail))
                if detail.contains("unexpected protobuf payload for exec_exit: file_read_response")
        ),
        "expected malformed-peer response type mismatch, got {err:?}"
    );
    assert_diagnostics_contain(
        &run_dir,
        "unexpected protobuf payload for exec_exit: file_read_response",
    );

    running
        .force_kill()
        .expect("force kill response-type-mismatch malicious VM")
        .delete()
        .expect("delete response-type-mismatch malicious VM");
}

fn assert_diagnostics_contain(run_dir: &std::path::Path, needle: &str) {
    let path = run_dir.join("diagnostics.jsonl");
    let diagnostics =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    assert!(
        diagnostics.contains(needle),
        "diagnostics did not contain {needle:?}:\n{diagnostics}"
    );
}
