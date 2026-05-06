//! Streaming exec payloads.

use serde::{Deserialize, Serialize};

use super::{b64, ExecStatus, ExecTiming, Payload};

/// Wire `kind` value for an envelope carrying [`ExecStdout`].
pub const PAYLOAD_KIND_EXEC_STDOUT: &str = "exec_stdout";

/// Wire `kind` value for an envelope carrying [`ExecStderr`].
pub const PAYLOAD_KIND_EXEC_STDERR: &str = "exec_stderr";

/// Wire `kind` value for an envelope carrying [`ExecExit`].
pub const PAYLOAD_KIND_EXEC_EXIT: &str = "exec_exit";

impl Payload for ExecStdout {
    const KIND: &'static str = PAYLOAD_KIND_EXEC_STDOUT;
}

impl Payload for ExecStderr {
    const KIND: &'static str = PAYLOAD_KIND_EXEC_STDERR;
}

impl Payload for ExecExit {
    const KIND: &'static str = PAYLOAD_KIND_EXEC_EXIT;
}

/// Standard-output chunk emitted by streaming exec.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecStdout {
    /// Monotonic sequence number within stdout for one request.
    pub seq: u32,
    /// Raw stdout bytes. On the wire: base64-encoded JSON string.
    #[serde(with = "b64::single")]
    pub bytes: Vec<u8>,
}

/// Standard-error chunk emitted by streaming exec.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecStderr {
    /// Monotonic sequence number within stderr for one request.
    pub seq: u32,
    /// Raw stderr bytes. On the wire: base64-encoded JSON string.
    #[serde(with = "b64::single")]
    pub bytes: Vec<u8>,
}

/// Terminal frame emitted after all streaming stdout/stderr chunks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecExit {
    /// Terminal status.
    pub status: ExecStatus,
    /// Process exit code when `status == Completed`. `None` when the process
    /// never produced an exit status.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    /// Total stdout bytes read by guestd before terminal completion.
    pub total_stdout_bytes: u64,
    /// Total stderr bytes read by guestd before terminal completion.
    pub total_stderr_bytes: u64,
    /// Guest-side truncation flag. Normal streaming emits `false`.
    pub truncated: bool,
    /// Timing for this execution.
    pub timing: ExecTiming,
}
