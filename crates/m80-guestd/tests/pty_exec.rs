//! PTY exec tests for the transport-agnostic connection handler.

use std::io::{BufRead, BufReader, Cursor};
use std::time::{Duration, Instant};

use m80_proto::{
    read_frame, write_frame, CancelAck, CancelRequest, CancelStatus, Envelope, ExecStatus,
    PtyControl, PtyControlEvent, PtyExit, PtyInput, PtyOutput, PtyRequest, PtyResize, PtySize,
    PAYLOAD_KIND_CANCEL_ACK, PAYLOAD_KIND_PTY_EXIT, PAYLOAD_KIND_PTY_OUTPUT,
};

fn pty_size(rows: u16, cols: u16) -> PtySize {
    PtySize {
        rows,
        cols,
        pixel_width: None,
        pixel_height: None,
    }
}

fn pty_request(program: &str, args: Vec<String>, timeout_ms: u64, size: PtySize) -> PtyRequest {
    PtyRequest {
        program: program.into(),
        args,
        cwd: None,
        env: None,
        timeout_ms: Some(timeout_ms),
        size,
    }
}

fn request_frame(req: PtyRequest, request_id: &str) -> Vec<u8> {
    let env = Envelope::with_request_id(req, request_id.to_owned());
    let mut buf = Vec::new();
    write_frame(&mut buf, &env).expect("write request frame");
    buf
}

fn input_frame(request_id: &str, seq: u32, bytes: &[u8]) -> Vec<u8> {
    let env = Envelope::with_request_id(
        PtyInput {
            seq,
            bytes: bytes.to_vec(),
        },
        request_id.to_owned(),
    );
    let mut buf = Vec::new();
    write_frame(&mut buf, &env).expect("write input frame");
    buf
}

fn resize_frame(request_id: &str, seq: u32, size: PtySize) -> Vec<u8> {
    let env = Envelope::with_request_id(PtyResize { seq, size }, request_id.to_owned());
    let mut buf = Vec::new();
    write_frame(&mut buf, &env).expect("write resize frame");
    buf
}

fn eof_frame(request_id: &str, seq: u32) -> Vec<u8> {
    let env = Envelope::with_request_id(
        PtyControl {
            seq,
            event: PtyControlEvent::Eof,
        },
        request_id.to_owned(),
    );
    let mut buf = Vec::new();
    write_frame(&mut buf, &env).expect("write eof frame");
    buf
}

fn cancel_frame(request_id: &str) -> Vec<u8> {
    let env = Envelope::new(CancelRequest {
        request_id: request_id.to_owned(),
    });
    let mut buf = Vec::new();
    write_frame(&mut buf, &env).expect("write cancel frame");
    buf
}

fn run_pty_with_open_reader(input: Vec<u8>) -> Vec<u8> {
    let mut out = Vec::new();
    m80_guestd::connection::handle_connection_with_reader_ready(
        BufReader::new(Cursor::new(input)),
        &mut out,
        |reader| reader.fill_buf().map(|buf| !buf.is_empty()).unwrap_or(true),
    )
    .expect("handle pty connection");
    out
}

fn run_pty_with_disconnect(input: Vec<u8>) -> Vec<u8> {
    let mut out = Vec::new();
    m80_guestd::connection::handle_connection_with_reader_ready(BufReader::new(Cursor::new(input)), &mut out, |_| true)
        .expect("handle pty connection");
    out
}

fn read_raw_frames(bytes: &[u8]) -> Vec<Envelope<serde_json::Value>> {
    let mut cursor = Cursor::new(bytes);
    let mut frames = Vec::new();
    while (cursor.position() as usize) < bytes.len() {
        frames.push(read_frame(&mut cursor).expect("read raw frame"));
    }
    frames
}

fn output_and_exit(bytes: &[u8]) -> (Vec<u8>, PtyExit) {
    let frames = read_raw_frames(bytes);
    assert!(
        !frames.is_empty(),
        "pty response must contain at least an exit frame"
    );
    assert_eq!(frames.last().unwrap().kind, PAYLOAD_KIND_PTY_EXIT);

    let mut output = Vec::new();
    for frame in &frames[..frames.len() - 1] {
        assert_eq!(frame.kind, PAYLOAD_KIND_PTY_OUTPUT);
        let chunk: PtyOutput =
            serde_json::from_value(frame.payload.clone()).expect("pty output payload");
        output.extend(chunk.bytes);
    }

    let exit: PtyExit =
        serde_json::from_value(frames.last().unwrap().payload.clone()).expect("pty exit payload");
    (output, exit)
}

#[test]
fn pty_echo_probe_round_trips_terminal_output() {
    let mut input = request_frame(
        pty_request("/bin/cat", vec![], 5_000, pty_size(24, 80)),
        "pty-echo",
    );
    input.extend(input_frame("pty-echo", 0, b"hello from pty\n"));
    input.extend(eof_frame("pty-echo", 1));

    let (output, exit) = output_and_exit(&run_pty_with_open_reader(input));

    assert_eq!(exit.status, ExecStatus::Completed);
    assert_eq!(exit.exit_code, Some(0));
    assert_eq!(exit.total_input_bytes, b"hello from pty\n".len() as u64);
    assert!(
        output
            .windows(b"hello from pty".len())
            .any(|window| window == b"hello from pty"),
        "terminal output should contain echoed input, got: {:?}",
        String::from_utf8_lossy(&output)
    );
}

#[test]
fn pty_initial_size_is_visible_to_child() {
    let input = request_frame(
        pty_request(
            "/bin/sh",
            vec!["-c".into(), "stty size".into()],
            5_000,
            pty_size(33, 101),
        ),
        "pty-size",
    );

    let (output, exit) = output_and_exit(&run_pty_with_open_reader(input));

    assert_eq!(exit.status, ExecStatus::Completed);
    assert!(
        String::from_utf8_lossy(&output).contains("33 101"),
        "stty should observe initial pty size, got: {:?}",
        String::from_utf8_lossy(&output)
    );
}

#[test]
fn pty_resize_event_updates_child_size() {
    let mut input = request_frame(
        pty_request(
            "/bin/sh",
            vec!["-c".into(), "sleep 0.2; stty size".into()],
            5_000,
            pty_size(24, 80),
        ),
        "pty-resize",
    );
    input.extend(resize_frame("pty-resize", 0, pty_size(44, 120)));

    let (output, exit) = output_and_exit(&run_pty_with_open_reader(input));

    assert_eq!(exit.status, ExecStatus::Completed);
    assert!(
        String::from_utf8_lossy(&output).contains("44 120"),
        "stty should observe resized pty, got: {:?}",
        String::from_utf8_lossy(&output)
    );
}

#[test]
fn pty_child_exit_status_is_reported() {
    let input = request_frame(
        pty_request(
            "/bin/sh",
            vec!["-c".into(), "exit 7".into()],
            5_000,
            pty_size(24, 80),
        ),
        "pty-exit",
    );

    let (_output, exit) = output_and_exit(&run_pty_with_open_reader(input));

    assert_eq!(exit.status, ExecStatus::Completed);
    assert_eq!(exit.exit_code, Some(7));
}

#[test]
fn pty_spawn_failure_returns_failed_exit() {
    let input = request_frame(
        pty_request(
            "/nonexistent/binary/that/does/not/exist",
            vec![],
            5_000,
            pty_size(24, 80),
        ),
        "pty-spawn-failure",
    );

    let (output, exit) = output_and_exit(&run_pty_with_open_reader(input));

    assert_eq!(exit.status, ExecStatus::Failed);
    assert!(
        String::from_utf8_lossy(&output).contains("spawn failed"),
        "spawn failure should be surfaced in pty output, got: {:?}",
        String::from_utf8_lossy(&output)
    );
}

#[test]
fn pty_cancel_request_kills_child_and_returns_ack() {
    let mut input = request_frame(
        pty_request("/bin/sleep", vec!["60".into()], 30_000, pty_size(24, 80)),
        "pty-cancel",
    );
    input.extend(cancel_frame("pty-cancel"));

    let out = run_pty_with_disconnect(input);
    let ack: Envelope<CancelAck> = read_frame(&mut Cursor::new(&out)).expect("read cancel ack");

    assert_eq!(ack.kind, PAYLOAD_KIND_CANCEL_ACK);
    assert_eq!(ack.payload.request_id, "pty-cancel");
    assert_eq!(ack.payload.status, CancelStatus::Cancelled);
}

#[test]
fn pty_reader_eof_kills_silent_child_without_exit_frame() {
    let input = request_frame(
        pty_request("/bin/sleep", vec!["60".into()], 30_000, pty_size(24, 80)),
        "pty-eof",
    );

    let start = Instant::now();
    let out = run_pty_with_disconnect(input);
    let elapsed = start.elapsed();

    assert!(
        elapsed < Duration::from_secs(5),
        "reader EOF must kill pty child promptly, elapsed={elapsed:?}"
    );
    assert!(
        out.is_empty(),
        "disconnect cancellation must not emit PtyExit; got {out:?}"
    );
}
