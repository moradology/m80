//! Streaming exec tests for the transport-agnostic connection handler.

use std::io::{self, BufReader, Cursor, Write};
use std::os::unix::net::UnixStream;
use std::thread;
use std::time::{Duration, Instant};

use m80_proto::{
    read_frame, read_raw_frame, write_frame, CancelRequest, CancelResponse, CancelStatus, Envelope,
    ExecExit, ExecRequest, ExecStatus, ExecStderr, ExecStdout, PAYLOAD_KIND_CANCEL_RESPONSE,
    PAYLOAD_KIND_EXEC_EXIT, PAYLOAD_KIND_EXEC_STDERR, PAYLOAD_KIND_EXEC_STDOUT,
};

fn streaming_request(program: &str, args: Vec<String>, timeout_ms: u64) -> ExecRequest {
    ExecRequest {
        program: program.into(),
        args,
        cwd: None,
        env: None,
        stdin: None,
        timeout_ms: Some(timeout_ms),
        streaming: true,
    }
}

fn request_frame(req: ExecRequest, request_id: &str) -> Vec<u8> {
    let env = Envelope::with_request_id(req, request_id.to_owned());
    let mut buf = Vec::new();
    write_frame(&mut buf, &env).expect("write request frame");
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

fn run_streaming_with_open_reader(input: Vec<u8>) -> Vec<u8> {
    let mut out = Vec::new();
    m80_guestd::connection::handle_connection_with_reader_ready(
        std::io::BufReader::new(Cursor::new(input)),
        &mut out,
        |_reader| false,
    )
    .expect("handle streaming connection");
    out
}

fn read_raw_frames(bytes: &[u8]) -> Vec<m80_proto::RawEnvelope> {
    let mut cursor = Cursor::new(bytes);
    let mut frames = Vec::new();
    while (cursor.position() as usize) < bytes.len() {
        frames.push(read_raw_frame(&mut cursor).expect("read raw frame"));
    }
    frames
}

fn assert_monotonic(seqs: &[u32]) {
    for (idx, seq) in seqs.iter().enumerate() {
        assert_eq!(*seq, idx as u32, "sequence gap at index {idx}");
    }
}

#[test]
fn streaming_stdout_stderr_chunks_end_with_exit() {
    let req = streaming_request(
        "/bin/sh",
        vec!["-c".into(), "printf out; printf err >&2; exit 7".into()],
        5_000,
    );
    let frames = read_raw_frames(&run_streaming_with_open_reader(request_frame(
        req, "stream-1",
    )));

    assert!(
        frames.len() >= 3,
        "expected stdout, stderr, and exit frames; got {frames:?}"
    );
    assert_eq!(frames.last().unwrap().kind, PAYLOAD_KIND_EXEC_EXIT);

    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let mut stdout_seqs = Vec::new();
    let mut stderr_seqs = Vec::new();

    for frame in &frames[..frames.len() - 1] {
        match frame.kind.as_str() {
            PAYLOAD_KIND_EXEC_STDOUT => {
                let chunk = frame
                    .clone()
                    .decode::<ExecStdout>()
                    .expect("stdout payload")
                    .payload;
                stdout_seqs.push(chunk.seq);
                stdout.extend(chunk.bytes);
            }
            PAYLOAD_KIND_EXEC_STDERR => {
                let chunk = frame
                    .clone()
                    .decode::<ExecStderr>()
                    .expect("stderr payload")
                    .payload;
                stderr_seqs.push(chunk.seq);
                stderr.extend(chunk.bytes);
            }
            other => panic!("unexpected streaming frame kind: {other}"),
        }
    }

    assert_monotonic(&stdout_seqs);
    assert_monotonic(&stderr_seqs);
    assert_eq!(stdout, b"out");
    assert_eq!(stderr, b"err");

    let exit = frames
        .last()
        .unwrap()
        .clone()
        .decode::<ExecExit>()
        .unwrap()
        .payload;
    assert_eq!(exit.status, ExecStatus::Completed);
    assert_eq!(exit.exit_code, Some(7));
    assert_eq!(exit.total_stdout_bytes, 3);
    assert_eq!(exit.total_stderr_bytes, 3);
    assert!(!exit.truncated);
}

#[test]
fn streaming_exec_rejects_oversized_stdin_before_spawn() {
    let mut req = streaming_request("cat", vec![], 5_000);
    req.stdin = Some(vec![b'x'; (1 << 20) + 1]);
    let frames = read_raw_frames(&run_streaming_with_open_reader(request_frame(
        req,
        "stream-oversized-stdin",
    )));

    assert_eq!(frames.len(), 2, "expected stderr plus terminal exit");
    assert_eq!(frames[0].kind, PAYLOAD_KIND_EXEC_STDERR);
    assert_eq!(frames[1].kind, PAYLOAD_KIND_EXEC_EXIT);
    let stderr = frames[0].clone().decode::<ExecStderr>().unwrap().payload;
    assert!(
        String::from_utf8_lossy(&stderr.bytes).contains("stdin payload too large"),
        "unexpected stderr chunk: {stderr:?}"
    );
    let exit = frames[1].clone().decode::<ExecExit>().unwrap().payload;
    assert_eq!(exit.status, ExecStatus::Failed);
}

#[test]
fn streaming_exec_output_is_not_capped_at_buffered_exec_limit() {
    let req = streaming_request(
        "/bin/sh",
        vec![
            "-c".into(),
            "dd if=/dev/zero bs=1024 count=1025 2>/dev/null".into(),
        ],
        5_000,
    );
    let frames = read_raw_frames(&run_streaming_with_open_reader(request_frame(
        req,
        "stream-large-output",
    )));

    let mut stdout_total = 0u64;
    for frame in &frames[..frames.len() - 1] {
        if frame.kind == PAYLOAD_KIND_EXEC_STDOUT {
            let chunk = frame.clone().decode::<ExecStdout>().unwrap().payload;
            stdout_total += chunk.bytes.len() as u64;
        }
    }
    let exit = frames
        .last()
        .unwrap()
        .clone()
        .decode::<ExecExit>()
        .unwrap()
        .payload;

    assert_eq!(exit.status, ExecStatus::Completed);
    assert_eq!(stdout_total, (1025 * 1024) as u64);
    assert_eq!(exit.total_stdout_bytes, stdout_total);
    assert!(!exit.truncated);
}

#[test]
fn streaming_no_output_still_sends_terminal_exit() {
    let req = streaming_request("/bin/true", vec![], 5_000);
    let frames = read_raw_frames(&run_streaming_with_open_reader(request_frame(
        req, "stream-2",
    )));

    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0].kind, PAYLOAD_KIND_EXEC_EXIT);
    let exit = frames[0].clone().decode::<ExecExit>().unwrap().payload;
    assert_eq!(exit.status, ExecStatus::Completed);
    assert_eq!(exit.exit_code, Some(0));
    assert_eq!(exit.total_stdout_bytes, 0);
    assert_eq!(exit.total_stderr_bytes, 0);
}

#[test]
fn streaming_repeated_execs_each_end_in_one_terminal_frame() {
    for i in 0..10 {
        let req = streaming_request("/bin/true", vec![], 5_000);
        let frames = read_raw_frames(&run_streaming_with_open_reader(request_frame(
            req,
            &format!("stream-repeat-{i}"),
        )));

        assert_eq!(frames.len(), 1, "request {i} must emit only terminal exit");
        assert_eq!(frames[0].kind, PAYLOAD_KIND_EXEC_EXIT);
        let exit = frames[0].clone().decode::<ExecExit>().unwrap().payload;
        assert_eq!(exit.status, ExecStatus::Completed);
        assert_eq!(exit.exit_code, Some(0));
    }
}

#[test]
fn streaming_cancel_request_kills_child_and_returns_ack() {
    let mut input = request_frame(
        streaming_request("/bin/sleep", vec!["60".into()], 30_000),
        "stream-cancel",
    );
    input.extend(cancel_frame("stream-cancel"));

    let mut out = Vec::new();
    m80_guestd::connection::handle_connection_with_reader_ready(
        std::io::BufReader::new(Cursor::new(input)),
        &mut out,
        |_| true,
    )
    .expect("handle streaming cancel");

    let ack: Envelope<CancelResponse> =
        read_frame(&mut Cursor::new(&out)).expect("read cancel ack");
    assert_eq!(ack.kind, PAYLOAD_KIND_CANCEL_RESPONSE);
    assert_eq!(ack.payload.request_id, "stream-cancel");
    assert_eq!(ack.payload.status, CancelStatus::Cancelled);
}

#[test]
fn streaming_cancel_request_kills_shell_spawned_grandchild() {
    let mut input = request_frame(
        streaming_request(
            "/bin/sh",
            vec!["-c".into(), "sleep 60 & wait".into()],
            30_000,
        ),
        "stream-cancel-grandchild",
    );
    input.extend(cancel_frame("stream-cancel-grandchild"));

    let start = Instant::now();
    let mut out = Vec::new();
    m80_guestd::connection::handle_connection_with_reader_ready(
        std::io::BufReader::new(Cursor::new(input)),
        &mut out,
        |_| true,
    )
    .expect("handle streaming cancel");
    let elapsed = start.elapsed();

    let ack: Envelope<CancelResponse> =
        read_frame(&mut Cursor::new(&out)).expect("read cancel ack");
    assert_eq!(ack.kind, PAYLOAD_KIND_CANCEL_RESPONSE);
    assert_eq!(ack.payload.request_id, "stream-cancel-grandchild");
    assert_eq!(ack.payload.status, CancelStatus::Cancelled);
    assert!(
        elapsed < Duration::from_secs(5),
        "process-group cancel should not wait for grandchild sleep timeout, elapsed={elapsed:?}"
    );
}

#[test]
fn streaming_reader_eof_kills_silent_child_without_exit_frame() {
    let input = request_frame(
        streaming_request("/bin/sleep", vec!["60".into()], 30_000),
        "stream-eof",
    );

    let start = Instant::now();
    let mut out = Vec::new();
    m80_guestd::connection::handle_connection_with_reader_ready(
        std::io::BufReader::new(Cursor::new(input)),
        &mut out,
        |_| true,
    )
    .expect("handle streaming eof");
    let elapsed = start.elapsed();

    assert!(
        elapsed < Duration::from_secs(5),
        "reader EOF must kill silent child promptly, elapsed={elapsed:?}"
    );
    assert!(
        out.is_empty(),
        "disconnect cancellation must not emit ExecExit; got {out:?}"
    );
}

#[test]
fn streaming_reader_eof_kills_shell_spawned_grandchild_promptly() {
    let input = request_frame(
        streaming_request(
            "/bin/sh",
            vec!["-c".into(), "sleep 60 & wait".into()],
            30_000,
        ),
        "stream-eof-grandchild",
    );

    let start = Instant::now();
    let mut out = Vec::new();
    m80_guestd::connection::handle_connection_with_reader_ready(
        std::io::BufReader::new(Cursor::new(input)),
        &mut out,
        |_| true,
    )
    .expect("handle streaming eof");
    let elapsed = start.elapsed();

    assert!(
        elapsed < Duration::from_secs(5),
        "reader EOF must kill grandchild promptly, elapsed={elapsed:?}"
    );
    assert!(
        out.is_empty(),
        "disconnect cancellation must not emit ExecExit; got {out:?}"
    );
}

#[test]
fn streaming_timeout_kills_shell_spawned_grandchild() {
    let req = streaming_request("/bin/sh", vec!["-c".into(), "sleep 60 & wait".into()], 100);

    let start = Instant::now();
    let frames = read_raw_frames(&run_streaming_with_open_reader(request_frame(
        req,
        "stream-timeout-grandchild",
    )));
    let elapsed = start.elapsed();

    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0].kind, PAYLOAD_KIND_EXEC_EXIT);
    let exit = frames[0].clone().decode::<ExecExit>().unwrap().payload;
    assert_eq!(exit.status, ExecStatus::TimedOut);
    assert!(
        elapsed < Duration::from_secs(5),
        "streaming timeout should not wait for grandchild sleep timeout, elapsed={elapsed:?}"
    );
}

struct BrokenWriter;

impl Write for BrokenWriter {
    fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
        Err(io::Error::new(
            io::ErrorKind::BrokenPipe,
            "test writer closed",
        ))
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn streaming_chunk_write_failure_kills_child_promptly() {
    let input = request_frame(
        streaming_request("yes", vec![], 30_000),
        "stream-broken-writer",
    );

    let start = Instant::now();
    m80_guestd::connection::handle_connection_with_reader_ready(
        std::io::BufReader::new(Cursor::new(input)),
        BrokenWriter,
        |_reader| false,
    )
    .expect("handler should swallow broken writer after cleanup");
    let elapsed = start.elapsed();

    assert!(
        elapsed < Duration::from_secs(5),
        "write failure must kill child promptly, elapsed={elapsed:?}"
    );
}

#[test]
#[ignore = "wall-clock smoke for real-time chunk arrival"]
fn streaming_stdout_frames_arrive_before_process_exit() {
    let (mut client, server) = UnixStream::pair().expect("unix stream pair");
    let server_writer = server.try_clone().expect("clone server writer");
    let server_thread = thread::spawn(move || {
        m80_guestd::connection::handle_connection_with_reader_ready(
            BufReader::new(server),
            server_writer,
            |_reader| false,
        )
        .expect("handle streaming unix connection");
    });

    let req = streaming_request(
        "/bin/sh",
        vec![
            "-c".into(),
            "for i in 1 2 3; do echo $i; sleep 0.25; done".into(),
        ],
        5_000,
    );
    let frame = request_frame(req, "stream-wall-clock");
    client.write_all(&frame).expect("write request");
    client.flush().expect("flush request");

    let start = Instant::now();
    let mut reader = BufReader::new(client);
    let first: Envelope<ExecStdout> = read_frame(&mut reader).expect("first stdout");
    let first_elapsed = start.elapsed();
    assert_eq!(first.kind, PAYLOAD_KIND_EXEC_STDOUT);
    assert_eq!(first.payload.seq, 0);

    let second: Envelope<ExecStdout> = read_frame(&mut reader).expect("second stdout");
    let third: Envelope<ExecStdout> = read_frame(&mut reader).expect("third stdout");
    let exit: Envelope<ExecExit> = read_frame(&mut reader).expect("exit");
    let exit_elapsed = start.elapsed();

    assert_eq!(second.payload.seq, 1);
    assert_eq!(third.payload.seq, 2);
    assert_eq!(exit.kind, PAYLOAD_KIND_EXEC_EXIT);
    assert_eq!(exit.payload.exit_code, Some(0));
    assert!(
        first_elapsed < Duration::from_millis(500),
        "first chunk should arrive before command completion: {first_elapsed:?}"
    );
    assert!(
        exit_elapsed >= Duration::from_millis(500),
        "exit should reflect the sleeps: {exit_elapsed:?}"
    );

    server_thread.join().expect("server thread");
}
