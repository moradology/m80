use std::sync::mpsc;
use std::time::Duration;

use m80_firecracker::{FcError, WireProtocolError};
use m80_proto::ExecRequest;

use super::common::RunDirDumpGuard;

#[test]
#[ignore = "requires KVM host and M80_MALICIOUS_ARTIFACT_DIR image built with m80-guestd-malicious"]
fn unsolicited_flood_fails_fast_without_host_memory_growth() {
    let (_backend, mut running, run_dir) = super::launch_malicious("unsolicited_flood");
    let _dump_guard = RunDirDumpGuard::new(run_dir.clone());
    let rss_before = current_rss_kb();

    let err = exec_error_with_timeout(&mut running, Duration::from_secs(10));

    assert!(
        matches!(
            err,
            FcError::Protocol(WireProtocolError::RequestIdMismatch {
                context: "exec exit",
                ref expected,
                got: Some(ref got),
            }) if expected.starts_with("malicious-unsolicited_flood")
                && expected.contains("-exec-")
                && got == "unsolicited-flood-0"
        ),
        "expected request-id mismatch on first unsolicited flood frame, got {err:?}"
    );
    let rss_after = current_rss_kb();
    assert!(
        rss_after.saturating_sub(rss_before) < 64 * 1024,
        "unsolicited flood should not grow host RSS without bound: rss before={rss_before} KiB after={rss_after} KiB"
    );
    assert_diagnostics_contain(&run_dir, "request_id mismatch in exec exit");
    assert_diagnostics_contain(&run_dir, "unsolicited-flood-0");

    running
        .force_kill()
        .expect("force kill unsolicited-flood malicious VM")
        .delete()
        .expect("delete unsolicited-flood malicious VM");
}

#[test]
#[ignore = "requires KVM host and M80_MALICIOUS_ARTIFACT_DIR image built with m80-guestd-malicious"]
fn slowloris_partial_frame_times_out_without_stuck_reader() {
    let (_backend, mut running, run_dir) = super::launch_malicious("slowloris");
    let _dump_guard = RunDirDumpGuard::new(run_dir.clone());

    let err = exec_error_with_timeout(&mut running, Duration::from_secs(12));

    assert!(
        matches!(
            err,
            FcError::Protocol(WireProtocolError::ReadTimeout {
                context: "streaming exec"
            })
        ),
        "expected protocol read timeout, got {err:?}"
    );
    assert_diagnostics_contain(&run_dir, "read timeout before terminal frame");

    running
        .force_kill()
        .expect("force kill slowloris malicious VM")
        .delete()
        .expect("delete slowloris malicious VM");
}

fn exec_error_with_timeout(
    running: &mut m80_firecracker::RunningSandbox,
    timeout: Duration,
) -> FcError {
    let (tx, rx) = mpsc::channel();
    std::thread::scope(|scope| {
        scope.spawn(|| {
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
                .expect_err("malicious DoS mode must fail");
            tx.send(err).expect("send exec error");
        });

        rx.recv_timeout(timeout)
            .expect("malicious DoS exec must not leave a stuck reader thread")
    })
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
