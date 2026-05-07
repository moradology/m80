//! Streaming exec payloads.

use super::{ExecStatus, ExecTiming};

/// Wire `kind` value for an envelope carrying [`ExecStdout`].
pub const PAYLOAD_KIND_EXEC_STDOUT: &str = "exec_stdout";

/// Wire `kind` value for an envelope carrying [`ExecStderr`].
pub const PAYLOAD_KIND_EXEC_STDERR: &str = "exec_stderr";

/// Wire `kind` value for an envelope carrying [`ExecExit`].
pub const PAYLOAD_KIND_EXEC_EXIT: &str = "exec_exit";

/// Standard-output chunk emitted by streaming exec.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecStdout {
    /// Monotonic sequence number within stdout for one request.
    pub seq: u32,
    /// Raw stdout bytes.
    pub bytes: Vec<u8>,
}

/// Standard-error chunk emitted by streaming exec.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecStderr {
    /// Monotonic sequence number within stderr for one request.
    pub seq: u32,
    /// Raw stderr bytes.
    pub bytes: Vec<u8>,
}

/// Terminal frame emitted after all streaming stdout/stderr chunks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecExit {
    /// Terminal status.
    pub status: ExecStatus,
    /// Process exit code when `status == Completed`. `None` when the process
    /// never produced an exit status.
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
