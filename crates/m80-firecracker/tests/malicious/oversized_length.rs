use m80_firecracker::{FcError, WireProtocolError};
use m80_proto::{ExecRequest, MAX_FRAME_BYTES};

use super::common::RunDirDumpGuard;

#[test]
#[ignore = "requires KVM host and M80_MALICIOUS_ARTIFACT_DIR image built with m80-guestd-malicious"]
fn oversized_length_prefix_returns_typed_protocol_error() {
    let (_backend, mut running, run_dir) = super::launch_malicious("oversized_length");
    let _dump_guard = RunDirDumpGuard::new(run_dir.clone());
    let rss_before = current_rss_kb();

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
        .expect_err("oversized malicious frame must fail");

    assert!(
        matches!(
            err,
            FcError::Protocol(WireProtocolError::OversizedFrame { size, limit })
                if size == MAX_FRAME_BYTES + 1 && limit == MAX_FRAME_BYTES
        ),
        "expected oversized-frame protocol error, got {err:?}"
    );
    let rss_after = current_rss_kb();
    assert!(
        rss_after.saturating_sub(rss_before) < 64 * 1024,
        "oversized prefix should not allocate a large body: rss before={rss_before} KiB after={rss_after} KiB"
    );
    assert_diagnostics_contain(&run_dir, "oversized frame");

    running
        .force_kill()
        .expect("force kill oversized malicious VM")
        .delete()
        .expect("delete oversized malicious VM");
}

fn current_rss_kb() -> u64 {
    let status = std::fs::read_to_string("/proc/self/status").expect("read /proc/self/status");
    status
        .lines()
        .find_map(|line| {
            line.strip_prefix("VmRSS:")
                .and_then(|rest| rest.split_whitespace().next())
                .and_then(|kb| kb.parse::<u64>().ok())
        })
        .expect("VmRSS in /proc/self/status")
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
