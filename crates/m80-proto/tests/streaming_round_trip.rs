use std::io::Cursor;

use m80_proto::{
    read_frame, write_frame, Envelope, ExecExit, ExecRequest, ExecStatus, ExecStderr, ExecStdout,
    PAYLOAD_KIND_EXEC_EXIT, PAYLOAD_KIND_EXEC_STDERR, PAYLOAD_KIND_EXEC_STDOUT,
};

mod common;

#[test]
fn stdout_chunk_round_trips() {
    let env = Envelope::with_request_id(
        ExecStdout {
            seq: 7,
            bytes: b"stdout bytes\n".to_vec(),
        },
        "req-stream-1".into(),
    );

    let mut buf = Vec::new();
    write_frame(&mut buf, &env).expect("write stdout chunk");
    let decoded: Envelope<ExecStdout> =
        read_frame(&mut Cursor::new(&buf)).expect("read stdout chunk");

    assert_eq!(decoded.kind, PAYLOAD_KIND_EXEC_STDOUT);
    assert_eq!(decoded.request_id.as_deref(), Some("req-stream-1"));
    assert_eq!(decoded.payload.seq, 7);
    assert_eq!(decoded.payload.bytes, b"stdout bytes\n");
}

#[test]
fn stderr_chunk_round_trips() {
    let env = Envelope::with_request_id(
        ExecStderr {
            seq: 3,
            bytes: b"stderr bytes\n".to_vec(),
        },
        "req-stream-2".into(),
    );

    let mut buf = Vec::new();
    write_frame(&mut buf, &env).expect("write stderr chunk");
    let decoded: Envelope<ExecStderr> =
        read_frame(&mut Cursor::new(&buf)).expect("read stderr chunk");

    assert_eq!(decoded.kind, PAYLOAD_KIND_EXEC_STDERR);
    assert_eq!(decoded.request_id.as_deref(), Some("req-stream-2"));
    assert_eq!(decoded.payload.seq, 3);
    assert_eq!(decoded.payload.bytes, b"stderr bytes\n");
}

#[test]
fn exit_frame_round_trips() {
    let env = Envelope::with_request_id(
        ExecExit {
            status: ExecStatus::Completed,
            exit_code: Some(17),
            total_stdout_bytes: 12,
            total_stderr_bytes: 5,
            truncated: false,
            timing: common::sample_timing(),
        },
        "req-stream-3".into(),
    );

    let mut buf = Vec::new();
    write_frame(&mut buf, &env).expect("write exit frame");
    let decoded: Envelope<ExecExit> = read_frame(&mut Cursor::new(&buf)).expect("read exit frame");

    assert_eq!(decoded.kind, PAYLOAD_KIND_EXEC_EXIT);
    assert_eq!(decoded.request_id.as_deref(), Some("req-stream-3"));
    assert_eq!(decoded.payload.status, ExecStatus::Completed);
    assert_eq!(decoded.payload.exit_code, Some(17));
    assert_eq!(decoded.payload.total_stdout_bytes, 12);
    assert_eq!(decoded.payload.total_stderr_bytes, 5);
    assert!(!decoded.payload.truncated);
}

#[test]
fn streaming_false_matches_v01_golden() {
    let env = Envelope::new(ExecRequest {
        program: "/bin/echo".into(),
        args: vec!["hi".into()],
        cwd: None,
        env: None,
        stdin: None,
        timeout_ms: Some(1_000),
        streaming: false,
    });

    let serialized = serde_json::to_vec(&env).expect("serialize request");
    let golden = include_str!("golden/exec_request_v01_streaming_false.json")
        .trim_end()
        .as_bytes()
        .to_vec();
    assert_eq!(serialized, golden);
}

#[test]
fn streaming_true_serializes_explicit_field() {
    let env = Envelope::new(ExecRequest {
        program: "/bin/echo".into(),
        args: vec!["hi".into()],
        cwd: None,
        env: None,
        stdin: None,
        timeout_ms: Some(1_000),
        streaming: true,
    });

    let serialized = serde_json::to_string(&env).expect("serialize request");
    assert!(
        serialized.contains("\"streaming\":true"),
        "streaming=true must be explicit in JSON: {serialized}"
    );
}
