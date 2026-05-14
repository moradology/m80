use std::io::{BufReader, Cursor};

use m80_proto::{
    read_frame, read_raw_frame, write_frame, Envelope, ExecExit, ExecRequest, ExecResponse,
    ExecStatus, ExecStderr, PtyExit, PtyOutput, PtyRequest, PtySize, PAYLOAD_KIND_EXEC_EXIT,
    PAYLOAD_KIND_EXEC_STDERR, PAYLOAD_KIND_PTY_EXIT, PAYLOAD_KIND_PTY_OUTPUT,
};

fn oversized_arg() -> String {
    "x".repeat((1 << 20) + 1)
}

fn exec_request(streaming: bool) -> ExecRequest {
    ExecRequest {
        program: "/bin/true".into(),
        args: vec![oversized_arg()],
        cwd: None,
        env: None,
        stdin: None,
        timeout_ms: Some(5_000),
        streaming,
    }
}

fn pty_request() -> PtyRequest {
    PtyRequest {
        program: "/bin/true".into(),
        args: vec![oversized_arg()],
        cwd: None,
        env: None,
        timeout_ms: Some(5_000),
        size: PtySize {
            rows: 24,
            cols: 80,
            pixel_width: None,
            pixel_height: None,
        },
    }
}

fn request_frame<T>(payload: T, request_id: &str) -> Vec<u8>
where
    T: m80_proto::Payload,
{
    let env = Envelope::with_request_id(payload, request_id.to_owned());
    let mut buf = Vec::new();
    write_frame(&mut buf, &env).expect("write request frame");
    buf
}

fn run_handler(input: Vec<u8>) -> Vec<u8> {
    let mut out = Vec::new();
    m80_guestd::connection::handle_connection_with_reader_ready(
        BufReader::new(Cursor::new(input)),
        &mut out,
        |_reader| false,
    )
    .expect("handle connection");
    out
}

fn raw_frames(bytes: &[u8]) -> Vec<m80_proto::RawEnvelope> {
    let mut cursor = Cursor::new(bytes);
    let mut frames = Vec::new();
    while (cursor.position() as usize) < bytes.len() {
        frames.push(read_raw_frame(&mut cursor).expect("read raw frame"));
    }
    frames
}

#[test]
fn buffered_exec_oversized_request_fails_at_broker_boundary() {
    let out = run_handler(request_frame(exec_request(false), "broker-buffered"));
    let mut cursor = Cursor::new(out);
    let env: Envelope<ExecResponse> = read_frame(&mut cursor).expect("read exec response");

    assert_eq!(env.payload.status, ExecStatus::Failed);
    let stderr = String::from_utf8_lossy(&env.payload.stderr);
    assert!(
        stderr.contains("workload broker request too large"),
        "unexpected stderr: {stderr}"
    );
}

#[test]
fn streaming_exec_oversized_request_fails_at_broker_boundary() {
    let frames = raw_frames(&run_handler(request_frame(
        exec_request(true),
        "broker-streaming",
    )));

    assert_eq!(frames.len(), 2, "unexpected frames: {frames:?}");
    assert_eq!(frames[0].kind, PAYLOAD_KIND_EXEC_STDERR);
    assert_eq!(frames[1].kind, PAYLOAD_KIND_EXEC_EXIT);

    let stderr = frames[0]
        .clone()
        .decode::<ExecStderr>()
        .expect("decode stderr")
        .payload;
    let exit = frames[1]
        .clone()
        .decode::<ExecExit>()
        .expect("decode exit")
        .payload;

    assert_eq!(exit.status, ExecStatus::Failed);
    let stderr = String::from_utf8_lossy(&stderr.bytes);
    assert!(
        stderr.contains("workload broker request too large"),
        "unexpected stderr: {stderr}"
    );
}

#[test]
fn pty_exec_oversized_request_fails_at_broker_boundary() {
    let frames = raw_frames(&run_handler(request_frame(pty_request(), "broker-pty")));

    assert_eq!(frames.len(), 2, "unexpected frames: {frames:?}");
    assert_eq!(frames[0].kind, PAYLOAD_KIND_PTY_OUTPUT);
    assert_eq!(frames[1].kind, PAYLOAD_KIND_PTY_EXIT);

    let output = frames[0]
        .clone()
        .decode::<PtyOutput>()
        .expect("decode pty output")
        .payload;
    let exit = frames[1]
        .clone()
        .decode::<PtyExit>()
        .expect("decode pty exit")
        .payload;

    assert_eq!(exit.status, ExecStatus::Failed);
    let output = String::from_utf8_lossy(&output.bytes);
    assert!(
        output.contains("workload broker request too large"),
        "unexpected pty output: {output}"
    );
}
