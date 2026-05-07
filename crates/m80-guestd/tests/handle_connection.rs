//! Tests for the transport-agnostic connection handler.
//!
//! Uses `std::io::Cursor` as the transport so no vsock socket is needed.
//! Real vsock tests require a VM context and are marked `#[ignore]`.

use std::io::Cursor;

use m80_proto::{
    read_frame, write_frame, CancelAck, CancelRequest, CancelStatus, Envelope, ExecRequest,
    ExecResponse, ExecStatus, PAYLOAD_KIND_CANCEL_ACK,
};

fn make_request(
    program: &str,
    args: Vec<String>,
    stdin: Option<Vec<u8>>,
    timeout_ms: u64,
) -> ExecRequest {
    ExecRequest {
        program: program.into(),
        args,
        cwd: None,
        env: None,
        stdin,
        timeout_ms: Some(timeout_ms),
        streaming: false,
    }
}

fn request_frame(req: ExecRequest, request_id: Option<&str>) -> Vec<u8> {
    let env = match request_id {
        Some(id) => Envelope::with_request_id(req, id.to_owned()),
        None => Envelope::new(req),
    };
    let mut buf = Vec::new();
    write_frame(&mut buf, &env).expect("write_frame in test");
    buf
}

fn cancel_frame(request_id: &str) -> Vec<u8> {
    let req = CancelRequest {
        request_id: request_id.to_owned(),
    };
    let env = Envelope::new(req);
    let mut buf = Vec::new();
    write_frame(&mut buf, &env).expect("write_frame in test");
    buf
}

fn read_response(bytes: &[u8]) -> Envelope<ExecResponse> {
    let mut cursor = Cursor::new(bytes);
    read_frame(&mut cursor).expect("read_frame in test")
}

fn read_cancel_ack(bytes: &[u8]) -> Envelope<CancelAck> {
    let mut cursor = Cursor::new(bytes);
    read_frame(&mut cursor).expect("read_frame in test")
}

fn run_handler(input: Vec<u8>) -> Vec<u8> {
    let mut out = Vec::new();
    m80_guestd::connection::handle_connection_with_reader_ready(
        std::io::BufReader::new(Cursor::new(input)),
        &mut out,
        |_| true,
    )
    .expect("handle_connection failed");
    out
}

fn run_handler_with_reader_never_ready(input: Vec<u8>) -> Vec<u8> {
    let mut out = Vec::new();
    m80_guestd::connection::handle_connection_with_reader_ready(
        std::io::BufReader::new(Cursor::new(input)),
        &mut out,
        |_reader| false,
    )
    .expect("handle_connection failed");
    out
}

#[test]
fn exec_true_returns_completed() {
    let input = request_frame(make_request("true", vec![], None, 5_000), None);
    let env = read_response(&run_handler(input));
    assert_eq!(env.payload.status, ExecStatus::Completed);
}

#[test]
fn exec_response_does_not_wait_for_cancel_readability() {
    let input = request_frame(make_request("true", vec![], None, 5_000), None);
    let env = read_response(&run_handler_with_reader_never_ready(input));
    assert_eq!(env.payload.status, ExecStatus::Completed);
}

#[test]
fn exec_with_stdin_round_trips() {
    let req = make_request("cat", vec![], Some(b"hello\n".to_vec()), 5_000);
    let env = read_response(&run_handler(request_frame(req, None)));
    assert_eq!(env.payload.status, ExecStatus::Completed);
    assert_eq!(env.payload.stdout, b"hello\n");
}

#[test]
fn exec_with_timeout_returns_timed_out() {
    let req = make_request("sleep", vec!["60".into()], None, 100);
    let start = std::time::Instant::now();
    let env = read_response(&run_handler(request_frame(req, None)));
    let elapsed = start.elapsed();
    assert_eq!(env.payload.status, ExecStatus::TimedOut);
    // `handle_connection` syncs before returning; on a busy host that can add
    // seconds. This still proves guestd did not wait for the full `sleep 60`.
    assert!(
        elapsed < std::time::Duration::from_secs(10),
        "elapsed: {elapsed:?}"
    );
}

#[test]
fn exec_failed_program_returns_failed() {
    let req = make_request(
        "/nonexistent/binary/that/does/not/exist",
        vec![],
        None,
        5_000,
    );
    let env = read_response(&run_handler(request_frame(req, None)));
    assert_eq!(env.payload.status, ExecStatus::Failed);
}

#[test]
fn exec_request_id_round_trips() {
    let req = make_request("true", vec![], None, 5_000);
    let env = read_response(&run_handler(request_frame(req, Some("req-1"))));
    assert_eq!(env.request_id, Some("req-1".to_owned()));
}

// ── Cancel tests ──────────────────────────────────────────────────────────────

/// Cancel while a long-running exec is in flight: the cancel frame is
/// pre-buffered after the exec frame. The handler reads exec, spawns sleep,
/// polls the buffer, sees the cancel, SIGKILLs, and replies with CancelAck.
#[test]
fn cancel_mid_exec_returns_cancelled_ack() {
    let mut input = request_frame(
        make_request("sleep", vec!["60".into()], None, 30_000),
        Some("req-cancel-1"),
    );
    input.extend(cancel_frame("req-cancel-1"));

    let out = run_handler(input);
    let ack = read_cancel_ack(&out);

    assert_eq!(ack.kind, PAYLOAD_KIND_CANCEL_ACK);
    assert_eq!(ack.payload.request_id, "req-cancel-1");
    assert_eq!(ack.payload.status, CancelStatus::Cancelled);
}

/// Cancel while a long-running exec is in flight terminates in well under
/// the exec's own timeout.
#[test]
fn cancel_mid_exec_terminates_quickly() {
    let mut input = request_frame(
        make_request("sleep", vec!["60".into()], None, 30_000),
        Some("req-cancel-timing"),
    );
    input.extend(cancel_frame("req-cancel-timing"));

    let start = std::time::Instant::now();
    run_handler(input);
    let elapsed = start.elapsed();

    assert!(
        elapsed < std::time::Duration::from_secs(5),
        "cancel should terminate well before sleep timeout, elapsed={elapsed:?}"
    );
}

#[test]
fn cancel_mid_exec_kills_shell_spawned_grandchild() {
    let mut input = request_frame(
        make_request(
            "/bin/sh",
            vec!["-c".into(), "sleep 60 & wait".into()],
            None,
            30_000,
        ),
        Some("req-cancel-grandchild"),
    );
    input.extend(cancel_frame("req-cancel-grandchild"));

    let start = std::time::Instant::now();
    let out = run_handler(input);
    let elapsed = start.elapsed();
    let ack = read_cancel_ack(&out);

    assert_eq!(ack.payload.status, CancelStatus::Cancelled);
    assert!(
        elapsed < std::time::Duration::from_secs(5),
        "process-group cancel should not wait for grandchild sleep timeout, elapsed={elapsed:?}"
    );
}

#[test]
fn malformed_control_frame_aborts_inflight_exec_quickly() {
    let mut input = request_frame(
        make_request("sleep", vec!["60".into()], None, 30_000),
        Some("req-malformed-control"),
    );
    input.extend_from_slice(&4u32.to_be_bytes());
    input.extend_from_slice(&[0xff, 0xff, 0xff, 0xff]);

    let start = std::time::Instant::now();
    let out = run_handler(input);
    let elapsed = start.elapsed();

    assert!(
        out.is_empty(),
        "malformed control should close without a response"
    );
    assert!(
        elapsed < std::time::Duration::from_secs(5),
        "malformed control should abort the in-flight exec, elapsed={elapsed:?}"
    );
}

#[test]
fn timeout_kills_shell_spawned_grandchild() {
    let req = make_request(
        "/bin/sh",
        vec!["-c".into(), "sleep 60 & wait".into()],
        None,
        100,
    );

    let start = std::time::Instant::now();
    let env = read_response(&run_handler(request_frame(req, None)));
    let elapsed = start.elapsed();

    assert_eq!(env.payload.status, ExecStatus::TimedOut);
    assert!(
        elapsed < std::time::Duration::from_secs(5),
        "process-group timeout should not wait for grandchild sleep timeout, elapsed={elapsed:?}"
    );
}

/// A cancel_request with a mismatched request_id while an exec is in flight
/// returns AlreadyExited (not Cancelled) and the exec continues normally.
#[test]
fn cancel_wrong_request_id_returns_already_exited() {
    // sleep with a short timeout so the test doesn't hang if something is wrong.
    let mut input = request_frame(
        make_request("sleep", vec!["60".into()], None, 200),
        Some("req-real"),
    );
    // Wrong ID — does not match the in-flight exec.
    input.extend(cancel_frame("req-bogus"));

    let out = run_handler(input);

    // The wrong-ID cancel produces an AlreadyExited ack.
    // After that the exec times out and produces its ExecResponse.
    // Both are written to `out`; read ack first, then response.
    let ack = read_cancel_ack(&out);
    assert_eq!(ack.payload.status, CancelStatus::AlreadyExited);
    assert_eq!(ack.payload.request_id, "req-bogus");

    // The exec itself timed out.
    let remaining = &out[{
        // Advance past the ack frame length.
        let mut cur = Cursor::new(&out);
        let _: Envelope<CancelAck> = read_frame(&mut cur).unwrap();
        cur.position() as usize
    }..];
    let exec_resp: Envelope<ExecResponse> = {
        let mut cur = Cursor::new(remaining);
        read_frame(&mut cur).expect("exec response after wrong-id cancel")
    };
    assert_eq!(exec_resp.payload.status, ExecStatus::TimedOut);
}

/// A cancel_request that arrives when no exec is in flight (standalone
/// cancel_request envelope) returns AlreadyExited.
#[test]
fn cancel_no_exec_in_flight_returns_already_exited() {
    let input = cancel_frame("req-orphan");
    let out = run_handler(input);
    let ack = read_cancel_ack(&out);
    assert_eq!(ack.payload.request_id, "req-orphan");
    assert_eq!(ack.payload.status, CancelStatus::AlreadyExited);
}
