//! Shared sample constructors for unit tests inside this crate.

use crate::types::{ExecRequest, ExecResponse, ExecStatus, ExecTiming};

pub fn sample_timing() -> ExecTiming {
    ExecTiming {
        spawned_at_unix_ms: 1_000_000,
        exited_at_unix_ms: 1_000_100,
        spawn_ms: 10,
        run_ms: 90,
    }
}

pub fn sample_request() -> ExecRequest {
    ExecRequest {
        program: "/bin/sh".into(),
        args: vec!["-c".into(), "echo hi".into()],
        cwd: None,
        env: None,
        stdin: None,
        timeout_ms: Some(5_000),
        streaming: false,
    }
}

pub fn sample_response() -> ExecResponse {
    ExecResponse {
        status: ExecStatus::Completed,
        exit_code: Some(0),
        stdout: b"hello\n".to_vec(),
        stderr: Vec::new(),
        truncated: None,
        timing: sample_timing(),
    }
}
