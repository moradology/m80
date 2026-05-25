use m80_firecracker::{FcError, WireProtocolError};
use m80_proto::ExecRequest;

use super::common::RunDirDumpGuard;

#[test]
#[ignore = "requires-kvm requires-artifacts requires-malicious-artifacts"]
fn unsolicited_response_is_rejected_as_request_id_mismatch_and_diagnosed() {
    let (_backend, mut running, run_dir) = super::launch_malicious("unsolicited_response");
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
        .expect_err("unsolicited response must fail the active request");

    assert!(
        matches!(
            err,
            FcError::Protocol(WireProtocolError::RequestIdMismatch {
                context: "exec exit",
                ref expected,
                got: Some(ref got),
            }) if expected.starts_with("malicious-unsolicited_response")
                && expected.contains("-exec-")
                && got == "unsolicited-response"
        ),
        "expected request-id mismatch for unsolicited response, got {err:?}"
    );
    assert_diagnostics_contain(&run_dir, "request_id mismatch in exec exit");
    assert_diagnostics_contain(&run_dir, "unsolicited-response");

    running
        .force_kill()
        .expect("force kill unsolicited-response malicious VM")
        .delete()
        .expect("delete unsolicited-response malicious VM");
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
