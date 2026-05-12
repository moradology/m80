//! CLI-owned JSON wrappers for `m80-proto` wire types.
//!
//! `m80-proto` deliberately stays transport-focused and does not depend on
//! serde. The CLI owns these shadow structs as its human/tool-facing JSON
//! surface, then converts at the command boundary before speaking protobuf-ish
//! host/guest wire types.

use serde::{Deserialize, Serialize};

use m80_proto::{ExecExit, ExecRequest, ExecResponse, ExecStatus, ExecTiming};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ExecRequestJson {
    pub program: String,
    pub args: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env: Option<Vec<(String, String)>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stdin: Option<Vec<u8>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
    pub streaming: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ExecResponseJson {
    pub status: ExecStatusJson,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub truncated: Option<bool>,
    pub timing: ExecTimingJson,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ExecExitJson {
    pub status: ExecStatusJson,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    pub total_stdout_bytes: u64,
    pub total_stderr_bytes: u64,
    pub truncated: bool,
    pub timing: ExecTimingJson,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ExecTimingJson {
    pub spawned_at_unix_ms: u64,
    pub exited_at_unix_ms: u64,
    pub spawn_ms: u64,
    pub run_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum ExecStatusJson {
    Completed,
    TimedOut,
    Cancelled,
    Failed,
}

impl From<ExecRequest> for ExecRequestJson {
    fn from(value: ExecRequest) -> Self {
        Self {
            program: value.program,
            args: value.args,
            cwd: value.cwd,
            env: value.env,
            stdin: value.stdin,
            timeout_ms: value.timeout_ms,
            streaming: value.streaming,
        }
    }
}

impl From<ExecRequestJson> for ExecRequest {
    fn from(value: ExecRequestJson) -> Self {
        Self {
            program: value.program,
            args: value.args,
            cwd: value.cwd,
            env: value.env,
            stdin: value.stdin,
            timeout_ms: value.timeout_ms,
            streaming: value.streaming,
        }
    }
}

impl From<ExecResponse> for ExecResponseJson {
    fn from(value: ExecResponse) -> Self {
        Self {
            status: value.status.into(),
            exit_code: value.exit_code,
            stdout: value.stdout,
            stderr: value.stderr,
            truncated: value.truncated,
            timing: value.timing.into(),
        }
    }
}

impl From<ExecResponseJson> for ExecResponse {
    fn from(value: ExecResponseJson) -> Self {
        Self {
            status: value.status.into(),
            exit_code: value.exit_code,
            stdout: value.stdout,
            stderr: value.stderr,
            truncated: value.truncated,
            timing: value.timing.into(),
        }
    }
}

impl From<ExecExit> for ExecExitJson {
    fn from(value: ExecExit) -> Self {
        Self {
            status: value.status.into(),
            exit_code: value.exit_code,
            total_stdout_bytes: value.total_stdout_bytes,
            total_stderr_bytes: value.total_stderr_bytes,
            truncated: value.truncated,
            timing: value.timing.into(),
        }
    }
}

impl From<ExecExitJson> for ExecExit {
    fn from(value: ExecExitJson) -> Self {
        Self {
            status: value.status.into(),
            exit_code: value.exit_code,
            total_stdout_bytes: value.total_stdout_bytes,
            total_stderr_bytes: value.total_stderr_bytes,
            truncated: value.truncated,
            timing: value.timing.into(),
        }
    }
}

impl From<ExecTiming> for ExecTimingJson {
    fn from(value: ExecTiming) -> Self {
        Self {
            spawned_at_unix_ms: value.spawned_at_unix_ms,
            exited_at_unix_ms: value.exited_at_unix_ms,
            spawn_ms: value.spawn_ms,
            run_ms: value.run_ms,
        }
    }
}

impl From<ExecTimingJson> for ExecTiming {
    fn from(value: ExecTimingJson) -> Self {
        Self {
            spawned_at_unix_ms: value.spawned_at_unix_ms,
            exited_at_unix_ms: value.exited_at_unix_ms,
            spawn_ms: value.spawn_ms,
            run_ms: value.run_ms,
        }
    }
}

impl From<ExecStatus> for ExecStatusJson {
    fn from(value: ExecStatus) -> Self {
        match value {
            ExecStatus::Completed => Self::Completed,
            ExecStatus::TimedOut => Self::TimedOut,
            ExecStatus::Cancelled => Self::Cancelled,
            ExecStatus::Failed => Self::Failed,
        }
    }
}

impl From<ExecStatusJson> for ExecStatus {
    fn from(value: ExecStatusJson) -> Self {
        match value {
            ExecStatusJson::Completed => Self::Completed,
            ExecStatusJson::TimedOut => Self::TimedOut,
            ExecStatusJson::Cancelled => Self::Cancelled,
            ExecStatusJson::Failed => Self::Failed,
        }
    }
}
