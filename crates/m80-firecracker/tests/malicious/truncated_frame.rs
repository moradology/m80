use std::sync::mpsc;
use std::time::Duration;

use m80_firecracker::{DisconnectCause, FcError, WireProtocolError};
use m80_proto::ExecRequest;

use super::common::RunDirDumpGuard;

#[test]
#[ignore = "requires KVM host and M80_MALICIOUS_ARTIFACT_DIR image built with m80-guestd-malicious"]
fn truncated_frame_returns_disconnect_before_terminal_without_stuck_reader() {
    let (_backend, mut running, run_dir) = super::launch_malicious("truncated_frame");
    let _dump_guard = RunDirDumpGuard::new(run_dir.clone());

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
                .expect_err("truncated malicious frame must fail");
            tx.send(err).expect("send exec error");
        });

        let err = rx
            .recv_timeout(Duration::from_secs(10))
            .expect("truncated-frame exec must not leave a stuck reader thread");
        assert!(
            matches!(
                err,
                FcError::Protocol(WireProtocolError::DisconnectBeforeTerminal {
                    context: "streaming exec",
                    cause: DisconnectCause::MidStreamEof
                })
            ),
            "expected disconnect-before-terminal protocol error, got {err:?}"
        );
    });

    assert_diagnostics_contain(&run_dir, "disconnect before terminal frame");
    running
        .force_kill()
        .expect("force kill truncated malicious VM")
        .delete()
        .expect("delete truncated malicious VM");
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
