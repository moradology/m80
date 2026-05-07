//! PTY exec payloads.

use super::{ExecStatus, ExecTiming};

/// Wire `kind` value for an envelope carrying [`PtyRequest`].
pub const PAYLOAD_KIND_PTY_REQUEST: &str = "pty_request";

/// Wire `kind` value for an envelope carrying [`PtyInput`].
pub const PAYLOAD_KIND_PTY_INPUT: &str = "pty_input";

/// Wire `kind` value for an envelope carrying [`PtyOutput`].
pub const PAYLOAD_KIND_PTY_OUTPUT: &str = "pty_output";

/// Wire `kind` value for an envelope carrying [`PtyResize`].
pub const PAYLOAD_KIND_PTY_RESIZE: &str = "pty_resize";

/// Wire `kind` value for an envelope carrying [`PtyControl`].
pub const PAYLOAD_KIND_PTY_CONTROL: &str = "pty_control";

/// Wire `kind` value for an envelope carrying [`PtyExit`].
pub const PAYLOAD_KIND_PTY_EXIT: &str = "pty_exit";

/// Terminal dimensions carried at PTY start and resize.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PtySize {
    /// Terminal rows.
    pub rows: u16,
    /// Terminal columns.
    pub cols: u16,
    /// Terminal pixel width when the host terminal reports it.
    pub pixel_width: Option<u16>,
    /// Terminal pixel height when the host terminal reports it.
    pub pixel_height: Option<u16>,
}

/// PTY exec request payload — sent from host to guest to start a terminal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PtyRequest {
    /// Executable path or name to spawn as the foreground terminal process.
    pub program: String,
    /// Arguments passed to `program` (not including `program` itself).
    pub args: Vec<String>,
    /// Working directory for the spawned process. `None` -> inherit from guest.
    pub cwd: Option<String>,
    /// Additional environment variables as `(key, value)` pairs.
    /// `None` -> inherit the guest's environment unchanged.
    pub env: Option<Vec<(String, String)>>,
    /// Wall-clock budget in milliseconds before the guest kills the process.
    /// `None` -> the guest applies its own default.
    pub timeout_ms: Option<u64>,
    /// Initial terminal size.
    pub size: PtySize,
}

/// Raw terminal-input bytes emitted by the host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PtyInput {
    /// Monotonic sequence number within terminal input for one request.
    pub seq: u32,
    /// Raw terminal bytes.
    pub bytes: Vec<u8>,
}

/// Raw terminal-output bytes emitted by the guest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PtyOutput {
    /// Monotonic sequence number within terminal output for one request.
    pub seq: u32,
    /// Raw terminal bytes.
    pub bytes: Vec<u8>,
}

/// Terminal resize event emitted by the host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PtyResize {
    /// Monotonic sequence number within host control events for one request.
    pub seq: u32,
    /// Updated terminal size.
    pub size: PtySize,
}

/// Terminal control event emitted by the host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PtyControl {
    /// Monotonic sequence number within host control events for one request.
    pub seq: u32,
    /// Control event to apply to the terminal session.
    pub event: PtyControlEvent,
}

/// Host-originated PTY control event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PtyControlEvent {
    /// Host terminal input reached EOF.
    Eof,
    /// Host wrapper asks guestd to signal the foreground process/session.
    Signal {
        /// Signal requested by the host wrapper.
        signal: PtySignal,
    },
}

/// Signals that the host wrapper may ask guestd to send to a PTY session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PtySignal {
    /// Interrupt signal.
    Interrupt,
    /// Termination signal.
    Terminate,
    /// Hangup signal.
    Hangup,
    /// Kill signal.
    Kill,
}

/// Terminal frame emitted after all PTY output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PtyExit {
    /// Terminal status.
    pub status: ExecStatus,
    /// Process exit code when available.
    pub exit_code: Option<i32>,
    /// Platform signal name when the process was terminated by a signal.
    pub exit_signal: Option<String>,
    /// Total terminal-input bytes read by guestd before completion.
    pub total_input_bytes: u64,
    /// Total terminal-output bytes read by guestd before terminal completion.
    pub total_output_bytes: u64,
    /// Guest-side truncation flag. Normal PTY streaming emits `false`.
    pub truncated: bool,
    /// Timing for this execution.
    pub timing: ExecTiming,
}
