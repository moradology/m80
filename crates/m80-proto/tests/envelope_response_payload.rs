//! Bead m80-g3x.1.3 — response envelope carries opaque exec outcome.

mod common;

use std::io::Cursor;

use m80_proto::{read_frame, write_frame, Envelope, ExecResponse, ExecStatus, PROTOCOL_VERSION};

fn round_trip(resp: ExecResponse) -> Envelope<ExecResponse> {
    let env = Envelope::new(resp);
    let mut buf = Vec::new();
    write_frame(&mut buf, &env).unwrap();
    let mut cursor = Cursor::new(buf);
    read_frame(&mut cursor).unwrap()
}

#[test]
fn completed_round_trips_with_exit_code_and_streams() {
    let resp = ExecResponse {
        status: ExecStatus::Completed,
        exit_code: Some(0),
        stdout: b"hello world\n".to_vec(),
        stderr: b"warning: deprecated\n".to_vec(),
        truncated: None,
        timing: common::sample_timing(),
    };
    let back = round_trip(resp);
    assert_eq!(back.version, PROTOCOL_VERSION);
    assert_eq!(back.payload.status, ExecStatus::Completed);
    assert_eq!(back.payload.exit_code, Some(0));
    assert_eq!(back.payload.stdout, b"hello world\n");
    assert_eq!(back.payload.stderr, b"warning: deprecated\n");
    assert_eq!(back.payload.timing.spawn_ms, 10);
    assert_eq!(back.payload.timing.run_ms, 90);
}

#[test]
fn timed_out_round_trips_without_exit_code() {
    let resp = ExecResponse {
        status: ExecStatus::TimedOut,
        exit_code: None,
        stdout: Vec::new(),
        stderr: b"killed by timeout\n".to_vec(),
        truncated: None,
        timing: common::sample_timing(),
    };
    let back = round_trip(resp);
    assert_eq!(back.payload.status, ExecStatus::TimedOut);
    assert_eq!(back.payload.exit_code, None);
}

#[test]
fn cancelled_round_trips_with_empty_streams() {
    let resp = ExecResponse {
        status: ExecStatus::Cancelled,
        exit_code: None,
        stdout: Vec::new(),
        stderr: Vec::new(),
        truncated: None,
        timing: common::sample_timing(),
    };
    let back = round_trip(resp);
    assert_eq!(back.payload.status, ExecStatus::Cancelled);
}

#[test]
fn failed_round_trips_with_error_stderr() {
    let resp = ExecResponse {
        status: ExecStatus::Failed,
        exit_code: Some(1),
        stdout: Vec::new(),
        stderr: b"exec: no such file\n".to_vec(),
        truncated: None,
        timing: common::sample_timing(),
    };
    let back = round_trip(resp);
    assert_eq!(back.payload.status, ExecStatus::Failed);
    assert_eq!(back.payload.exit_code, Some(1));
}
