use m80_firecracker::{FcError, WireProtocolError};
use m80_proto::ExecRequest;

use super::common::RunDirDumpGuard;

#[test]
#[ignore = "requires KVM host and M80_MALICIOUS_ARTIFACT_DIR image built with m80-guestd-malicious"]
fn unknown_variant_tag_returns_malformed_peer_with_field_number() {
    let (_backend, mut running, run_dir) = super::launch_malicious("unknown_variant");
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
        .expect_err("unknown payload variant must fail");

    assert!(
        matches!(
            err,
            FcError::Protocol(WireProtocolError::MalformedPeer(ref detail))
                if detail.contains("unknown envelope field: 255")
        ),
        "expected malformed peer with unknown field detail, got {err:?}"
    );
    assert_diagnostics_contain(&run_dir, "unknown envelope field: 255");

    running
        .force_kill()
        .expect("force kill unknown-variant malicious VM")
        .delete()
        .expect("delete unknown-variant malicious VM");
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
