//! Tests for the transport-agnostic connection handler.
//!
//! Uses `std::io::Cursor` as the transport so no vsock socket is needed.
//! Real vsock tests require a VM context and are marked `#[ignore]`.

use std::io::Cursor;

use m80_proto::{Envelope, ExecRequest, ExecResponse, ExecStatus, read_frame, write_frame};

/// Build a framed request into a byte buffer, return it as a Cursor.
fn request_frame(req: ExecRequest, request_id: Option<&str>) -> Vec<u8> {
    let env = match request_id {
        Some(id) => Envelope::with_request_id(req, id.to_owned()),
        None => Envelope::new(req),
    };
    let mut buf = Vec::new();
    write_frame(&mut buf, &env).expect("write_frame in test");
    buf
}

/// Read the response envelope from bytes written by the handler.
fn read_response(bytes: &[u8]) -> Envelope<ExecResponse> {
    let mut cursor = Cursor::new(bytes);
    read_frame(&mut cursor).expect("read_frame in test")
}

/// Drive handle_connection with the given request bytes; return written bytes.
fn run_handler(input: Vec<u8>) -> Vec<u8> {
    let mut out = Vec::new();
    m80_guestd::connection::handle_connection(
        std::io::BufReader::new(Cursor::new(input)),
        &mut out,
    )
    .expect("handle_connection failed");
    out
}

#[test]
fn exec_true_returns_completed() {
    let req = ExecRequest {
        program: "true".into(),
        args: vec![],
        cwd: None,
        env: None,
        stdin: None,
        workspace_dir: None,
        timeout_ms: Some(5_000),
    };
    let input = request_frame(req, None);
    let output = run_handler(input);
    let env = read_response(&output);
    assert_eq!(env.payload.status, ExecStatus::Completed);
    assert_eq!(env.payload.exit_code, Some(0));
}

#[test]
fn exec_with_stdin_round_trips() {
    let req = ExecRequest {
        program: "cat".into(),
        args: vec![],
        cwd: None,
        env: None,
        stdin: Some(b"hello\n".to_vec()),
        workspace_dir: None,
        timeout_ms: Some(5_000),
    };
    let input = request_frame(req, None);
    let output = run_handler(input);
    let env = read_response(&output);
    assert_eq!(env.payload.status, ExecStatus::Completed);
    assert_eq!(env.payload.stdout, b"hello\n");
}

#[test]
fn exec_with_timeout_returns_timed_out() {
    let req = ExecRequest {
        program: "sleep".into(),
        args: vec!["60".into()],
        cwd: None,
        env: None,
        stdin: None,
        workspace_dir: None,
        timeout_ms: Some(100),
    };
    let input = request_frame(req, None);
    let start = std::time::Instant::now();
    let output = run_handler(input);
    let elapsed = start.elapsed();
    let env = read_response(&output);
    assert_eq!(env.payload.status, ExecStatus::TimedOut);
    assert!(elapsed < std::time::Duration::from_millis(2_000), "elapsed: {elapsed:?}");
}

#[test]
fn exec_failed_program_returns_failed() {
    let req = ExecRequest {
        program: "/nonexistent/binary/that/does/not/exist".into(),
        args: vec![],
        cwd: None,
        env: None,
        stdin: None,
        workspace_dir: None,
        timeout_ms: Some(5_000),
    };
    let input = request_frame(req, None);
    let output = run_handler(input);
    let env = read_response(&output);
    assert_eq!(env.payload.status, ExecStatus::Failed);
}

#[test]
fn exec_request_id_round_trips() {
    let req = ExecRequest {
        program: "true".into(),
        args: vec![],
        cwd: None,
        env: None,
        stdin: None,
        workspace_dir: None,
        timeout_ms: Some(5_000),
    };
    let input = request_frame(req, Some("req-1"));
    let output = run_handler(input);
    let env = read_response(&output);
    assert_eq!(env.request_id, Some("req-1".to_owned()));
}
