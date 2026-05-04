//! Tests for the transport-agnostic connection handler.
//!
//! Uses `std::io::Cursor` as the transport so no vsock socket is needed.
//! Real vsock tests require a VM context and are marked `#[ignore]`.

use std::io::Cursor;

use m80_proto::{Envelope, ExecRequest, ExecResponse, ExecStatus, read_frame, write_frame};

fn make_request(program: &str, args: Vec<String>, stdin: Option<Vec<u8>>, timeout_ms: u64) -> ExecRequest {
    ExecRequest {
        program: program.into(),
        args,
        cwd: None,
        env: None,
        stdin,
        timeout_ms: Some(timeout_ms),
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

fn read_response(bytes: &[u8]) -> Envelope<ExecResponse> {
    let mut cursor = Cursor::new(bytes);
    read_frame(&mut cursor).expect("read_frame in test")
}

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
    let input = request_frame(make_request("true", vec![], None, 5_000), None);
    let env = read_response(&run_handler(input));
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
    assert!(elapsed < std::time::Duration::from_millis(2_000), "elapsed: {elapsed:?}");
}

#[test]
fn exec_failed_program_returns_failed() {
    let req = make_request("/nonexistent/binary/that/does/not/exist", vec![], None, 5_000);
    let env = read_response(&run_handler(request_frame(req, None)));
    assert_eq!(env.payload.status, ExecStatus::Failed);
}

#[test]
fn exec_request_id_round_trips() {
    let req = make_request("true", vec![], None, 5_000);
    let env = read_response(&run_handler(request_frame(req, Some("req-1"))));
    assert_eq!(env.request_id, Some("req-1".to_owned()));
}
