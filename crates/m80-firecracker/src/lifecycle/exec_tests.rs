use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;

use super::*;

#[test]
fn append_capped_reports_truncation_after_limit() {
    let mut dst = vec![b'a'; EXEC_BUFFER_LIMIT - 2];
    assert!(append_capped(&mut dst, b"abcd"));
    assert_eq!(dst.len(), EXEC_BUFFER_LIMIT);
    assert_eq!(&dst[EXEC_BUFFER_LIMIT - 2..], b"ab");
}

#[test]
fn append_capped_does_not_mark_empty_extra_as_truncated() {
    let mut dst = vec![b'a'; EXEC_BUFFER_LIMIT];
    assert!(!append_capped(&mut dst, b""));
    assert!(append_capped(&mut dst, b"x"));
}

#[test]
fn exec_open_retry_classifies_handshake_failure_as_transient() {
    assert!(is_transient_exec_open_send_error(
        &m80_vsock::VsockError::HandshakeFailed
    ));
}

#[test]
fn exec_open_retry_classifies_broken_pipe_as_transient() {
    let err = m80_vsock::VsockError::Io {
        path: std::path::PathBuf::new(),
        source: io::Error::new(io::ErrorKind::BrokenPipe, "closed"),
    };
    assert!(is_transient_exec_open_send_error(&err));
}

#[test]
fn exec_open_retry_classifies_connection_refused_as_transient() {
    let err = m80_vsock::VsockError::Io {
        path: std::path::PathBuf::new(),
        source: io::Error::new(io::ErrorKind::ConnectionRefused, "not yet listening"),
    };
    assert!(is_transient_exec_open_send_error(&err));
}

#[test]
fn exec_open_retry_does_not_retry_receive_style_timeouts() {
    let err = m80_vsock::VsockError::Io {
        path: std::path::PathBuf::new(),
        source: io::Error::new(io::ErrorKind::TimedOut, "timeout"),
    };
    assert!(!is_transient_exec_open_send_error(&err));
}

#[test]
fn host_exec_deadline_preserves_requested_budget() {
    let deadline = host_exec_deadline(60_000);

    assert_eq!(deadline.timeout, Duration::from_secs(60));
    assert!(deadline.deadline >= Instant::now());
}

#[test]
fn recv_raw_for_exec_returns_host_timeout_on_slow_drip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("vsock.sock");
    let listener = UnixListener::bind(&path).unwrap();

    let server = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        assert!(line.starts_with("CONNECT "));
        reader.get_mut().write_all(b"OK 9001\n").unwrap();

        for byte in [0, 0, 0, 8, b's', b'l', b'o', b'w'] {
            let _ = reader.get_mut().write_all(&[byte]);
            let _ = reader.get_mut().flush();
            std::thread::sleep(Duration::from_millis(30));
        }
    });

    let mut channel = Channel::open_uds_only(&path, GUEST_PORT_DEFAULT).unwrap();
    let started = Instant::now();
    let err = recv_raw_for_exec(&mut channel, Some(host_exec_deadline(50))).unwrap_err();

    assert!(matches!(
        err,
        FcError::ExecTimeoutHost { timeout } if timeout == Duration::from_millis(50)
    ));
    assert!(started.elapsed() < Duration::from_secs(1));

    drop(channel);
    server.join().unwrap();
}

#[test]
fn request_id_uses_caller_value_as_prefix_when_present() {
    // Caller's request_id is used as a base; a per-exec suffix is appended
    // so back-to-back execs on the same RunningSandbox don't collide on
    // the guest's cancel-routing table.
    let id = request_id_for("vm-1", Some("req-cli"), "exec");
    assert!(id.starts_with("req-cli-exec-"), "got {id}");
    let next = request_id_for("vm-1", Some("req-cli"), "exec");
    assert_ne!(id, next, "consecutive calls must produce distinct ids");
}

#[test]
fn request_id_generates_from_vm_and_kind_without_caller_value() {
    let request_id = request_id_for("vm-1", None, "pty");

    assert!(request_id.starts_with("vm-1-pty-"));
}

#[test]
fn cancelled_exit_reports_host_observed_cancel_status() {
    let exit = cancelled_exit(1_000, 7, 11);

    assert_eq!(exit.status, ExecStatus::Cancelled);
    assert_eq!(exit.exit_code, None);
    assert_eq!(exit.total_stdout_bytes, 7);
    assert_eq!(exit.total_stderr_bytes, 11);
    assert!(exit.timing.exited_at_unix_ms >= 1_000);
}

#[test]
fn non_one_shot_exec_claim_never_consumes() {
    let mut consumed = false;

    claim_one_shot_exec(false, &mut consumed).unwrap();
    claim_one_shot_exec(false, &mut consumed).unwrap();

    assert!(!consumed);
}

#[test]
fn one_shot_exec_claim_allows_only_first_workload() {
    let mut consumed = false;

    claim_one_shot_exec(true, &mut consumed).unwrap();
    let err = claim_one_shot_exec(true, &mut consumed).unwrap_err();

    assert!(matches!(err, FcError::OneShotConsumed));
    assert!(consumed);
}

#[test]
fn stream_sequence_accepts_monotonic_chunks() {
    let mut expected = 0;

    check_stream_sequence("exec_stdout", &mut expected, 0).unwrap();
    check_stream_sequence("exec_stdout", &mut expected, 1).unwrap();

    assert_eq!(expected, 2);
}

#[test]
fn stream_sequence_gap_returns_protocol_error() {
    let mut expected = 0;
    let err = check_stream_sequence("exec_stderr", &mut expected, 2).unwrap_err();

    assert!(matches!(
        err,
        FcError::Protocol(crate::error::WireProtocolError::SequenceMismatch {
            stream: "exec_stderr",
            expected: 0,
            got: 2
        })
    ));
    assert_eq!(expected, 0);
}

#[test]
fn stream_sequence_duplicate_returns_protocol_error() {
    let mut expected = 0;
    check_stream_sequence("pty_output", &mut expected, 0).unwrap();
    let err = check_stream_sequence("pty_output", &mut expected, 0).unwrap_err();

    assert!(matches!(
        err,
        FcError::Protocol(crate::error::WireProtocolError::SequenceMismatch {
            stream: "pty_output",
            expected: 1,
            got: 0
        })
    ));
}

#[test]
fn response_frame_rejects_stale_request_id() {
    let envelope = Envelope::with_request_id(
        ExecStdout {
            seq: 0,
            bytes: b"wrong request".to_vec(),
        },
        "stale-req".to_owned(),
    );
    let frame = RawEnvelope::from_typed(&envelope);

    let err =
        decode_frame_for_request::<ExecStdout>(frame, "active-req", "exec stdout").unwrap_err();

    assert!(matches!(
        err,
        FcError::Protocol(crate::error::WireProtocolError::RequestIdMismatch {
            context: "exec stdout",
            expected,
            got: Some(got),
        }) if expected == "active-req" && got == "stale-req"
    ));
}

#[test]
fn response_frame_rejects_missing_request_id() {
    let envelope = Envelope::new(ExecExit {
        status: ExecStatus::Completed,
        exit_code: Some(0),
        total_stdout_bytes: 0,
        total_stderr_bytes: 0,
        truncated: false,
        timing: ExecTiming {
            spawned_at_unix_ms: 1,
            exited_at_unix_ms: 2,
            spawn_ms: 0,
            run_ms: 1,
        },
    });
    let frame = RawEnvelope::from_typed(&envelope);

    let err = decode_frame_for_request::<ExecExit>(frame, "active-req", "exec exit").unwrap_err();

    assert!(matches!(
        err,
        FcError::Protocol(crate::error::WireProtocolError::RequestIdMismatch {
            context: "exec exit",
            expected,
            got: None,
        }) if expected == "active-req"
    ));
}

#[test]
fn cancel_ack_rejects_stale_request_id() {
    let envelope = Envelope::new(CancelResponse {
        request_id: "stale-req".to_owned(),
        status: CancelStatus::Cancelled,
    });
    let frame = RawEnvelope::from_typed(&envelope);

    let err = decode_cancel_ack(frame, "exec", "active-req").unwrap_err();

    assert!(matches!(
        err,
        FcError::Protocol(crate::error::WireProtocolError::RequestIdMismatch {
            context: "exec",
            expected,
            got: Some(got),
        }) if expected == "active-req" && got == "stale-req"
    ));
}

#[test]
fn cancel_forwarder_drop_detaches_after_join_timeout() {
    let stop = Arc::new(AtomicBool::new(false));
    let (done_tx, done_rx) = mpsc::channel();
    let (thread_released_tx, thread_released_rx) = mpsc::channel();
    let handle = std::thread::spawn(move || {
        let _done = ForwarderDone(done_tx);
        std::thread::sleep(FORWARDER_JOIN_TIMEOUT * 3);
        let _ = thread_released_tx.send(());
    });
    let forwarder = CancelForwarder {
        stop,
        done_rx,
        handle: Some(handle),
    };

    let started = Instant::now();
    drop(forwarder);

    assert!(
        started.elapsed() < FORWARDER_JOIN_TIMEOUT * 2,
        "drop waited for a blocked forwarder instead of detaching"
    );
    thread_released_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("detached test thread should still finish promptly");
}

#[test]
fn cancel_ack_failed_records_request_diagnostics() {
    let dir = tempfile::tempdir().unwrap();
    let mut diagnostics = Some(m80_observability::Diagnostics::open(dir.path()).unwrap());

    record_cancel_ack_failed(
        &mut diagnostics,
        "vm-test",
        "req-cancel",
        "exec",
        "guest failed to cancel exec request req-cancel",
        Duration::from_millis(7),
    );
    drop(diagnostics);

    let text =
        std::fs::read_to_string(dir.path().join(m80_observability::DIAGNOSTICS_FILE_NAME)).unwrap();
    assert!(text.contains("\"phase\":\"Request\""));
    assert!(text.contains("\"request_id\":\"req-cancel\""));
    assert!(text.contains("guest failed to cancel exec request req-cancel"));
}
