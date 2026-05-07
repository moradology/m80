//! Wire types: envelope, exec request/response, status, timing, handshake.

mod fileops;
mod metrics;
mod pty;
mod streaming;

pub use fileops::{
    DirEntry, FileError, FileKind, FileListRequest, FileListResponse, FileReadChunk,
    FileReadRequest, FileReadResponse, FileRemoveRequest, FileRemoveResponse, FileStat,
    FileStatRequest, FileStatResponse, FileWriteBeginRequest, FileWriteBeginResponse,
    FileWriteChunkRequest, FileWriteChunkResponse, FileWriteCommitRequest, FileWriteCommitResponse,
    FileWriteRequest, FileWriteResponse, FILE_READ_LIMIT_DEFAULT, PAYLOAD_KIND_FILE_LIST_REQUEST,
    PAYLOAD_KIND_FILE_LIST_RESPONSE, PAYLOAD_KIND_FILE_READ_CHUNK, PAYLOAD_KIND_FILE_READ_REQUEST,
    PAYLOAD_KIND_FILE_READ_RESPONSE, PAYLOAD_KIND_FILE_REMOVE_REQUEST,
    PAYLOAD_KIND_FILE_REMOVE_RESPONSE, PAYLOAD_KIND_FILE_STAT_REQUEST,
    PAYLOAD_KIND_FILE_STAT_RESPONSE, PAYLOAD_KIND_FILE_WRITE_BEGIN_REQUEST,
    PAYLOAD_KIND_FILE_WRITE_BEGIN_RESPONSE, PAYLOAD_KIND_FILE_WRITE_CHUNK_REQUEST,
    PAYLOAD_KIND_FILE_WRITE_CHUNK_RESPONSE, PAYLOAD_KIND_FILE_WRITE_COMMIT_REQUEST,
    PAYLOAD_KIND_FILE_WRITE_COMMIT_RESPONSE, PAYLOAD_KIND_FILE_WRITE_REQUEST,
    PAYLOAD_KIND_FILE_WRITE_RESPONSE,
};
pub use metrics::{
    GuestCpuMetrics, GuestMemMetrics, MetricsRequest, MetricsResponse,
    PAYLOAD_KIND_METRICS_REQUEST, PAYLOAD_KIND_METRICS_RESPONSE,
};
pub use pty::{
    PtyControl, PtyControlEvent, PtyExit, PtyInput, PtyOutput, PtyRequest, PtyResize, PtySignal,
    PtySize, PAYLOAD_KIND_PTY_CONTROL, PAYLOAD_KIND_PTY_EXIT, PAYLOAD_KIND_PTY_INPUT,
    PAYLOAD_KIND_PTY_OUTPUT, PAYLOAD_KIND_PTY_REQUEST, PAYLOAD_KIND_PTY_RESIZE,
};
pub use streaming::{
    ExecExit, ExecStderr, ExecStdout, PAYLOAD_KIND_EXEC_EXIT, PAYLOAD_KIND_EXEC_STDERR,
    PAYLOAD_KIND_EXEC_STDOUT,
};

use crate::version::PROTOCOL_VERSION;

/// Wire `kind` value for an envelope carrying [`ExecRequest`].
pub const PAYLOAD_KIND_EXEC_REQUEST: &str = "exec_request";

/// Wire `kind` value for an envelope carrying [`ExecResponse`].
pub const PAYLOAD_KIND_EXEC_RESPONSE: &str = "exec_response";

/// Wire `kind` value for an envelope carrying [`ShutdownRequest`].
pub const PAYLOAD_KIND_SHUTDOWN_REQUEST: &str = "shutdown_request";

/// Wire `kind` value for an envelope carrying [`ShutdownResponse`].
pub const PAYLOAD_KIND_SHUTDOWN_RESPONSE: &str = "shutdown_response";

/// Wire `kind` value for an envelope carrying [`CancelRequest`].
pub const PAYLOAD_KIND_CANCEL_REQUEST: &str = "cancel_request";

/// Wire `kind` value for an envelope carrying [`CancelResponse`].
pub const PAYLOAD_KIND_CANCEL_RESPONSE: &str = "cancel_response";

/// Marker trait for types that have a canonical protobuf wire `kind`.
pub trait Payload: Sized {
    /// Wire `kind` value, stamped into [`Envelope::kind`] by the constructors.
    const KIND: &'static str;
    /// Convert this public payload into the active protobuf payload variant.
    fn into_wire(self) -> crate::wire::WirePayload;
    /// Convert a protobuf payload variant into this public payload.
    fn from_wire(payload: crate::wire::WirePayload) -> Result<Self, crate::ProtoError>;
}

/// Wire envelope wrapping an opaque payload.
///
/// `version` fails closed at the framing layer. `kind` is a dispatch and
/// diagnostics label that must match the protobuf payload variant.
/// `request_id` is opaque to the protocol; consumers attach meaning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Envelope<T> {
    /// Protocol version. Must equal `PROTOCOL_VERSION` on every frame.
    pub version: u32,
    /// Payload-type discriminator. Stamped by [`Envelope::new`] /
    /// [`Envelope::with_request_id`] using `T::KIND`.
    pub kind: String,
    /// Optional caller-supplied identifier echoed back in the response.
    pub request_id: Option<String>,
    /// Payload; opaque from the protocol's perspective.
    pub payload: T,
}

impl<T: Payload> Envelope<T> {
    /// Construct a new envelope stamped with `PROTOCOL_VERSION` and `T::KIND`.
    pub fn new(payload: T) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            kind: T::KIND.to_owned(),
            request_id: None,
            payload,
        }
    }

    /// Construct a new envelope with a caller-supplied `request_id`.
    pub fn with_request_id(payload: T, request_id: String) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            kind: T::KIND.to_owned(),
            request_id: Some(request_id),
            payload,
        }
    }
}

/// Exec request payload — sent from host to guest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecRequest {
    /// Executable path or name to spawn.
    pub program: String,
    /// Arguments passed to `program` (not including `program` itself).
    pub args: Vec<String>,
    /// Working directory for the spawned process. `None` → inherit from guest.
    pub cwd: Option<String>,
    /// Additional environment variables as `(key, value)` pairs.
    /// `None` → inherit the guest's environment unchanged.
    pub env: Option<Vec<(String, String)>>,
    /// Bytes to feed to the process on stdin. `None` → stdin is closed.
    pub stdin: Option<Vec<u8>>,
    /// Wall-clock budget in milliseconds before the guest kills the process.
    /// `None` → the guest applies its own default.
    pub timeout_ms: Option<u64>,
    /// Opt into multi-frame stdout/stderr streaming.
    pub streaming: bool,
}

/// Terminal status of an exec operation. Adding a variant requires a
/// `PROTOCOL_VERSION` bump.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecStatus {
    /// Process exited; see `exit_code` for the code.
    Completed,
    /// Guest killed the process because it exceeded `timeout_ms`.
    TimedOut,
    /// Host disconnected mid-exec or otherwise requested cancellation. The
    /// in-VM `m80-guestd` daemon does not produce this status itself —
    /// it's set host-side when the orchestrator decides a run was
    /// cancelled (e.g., the host TCP-level connection dropped before the
    /// response was received).
    Cancelled,
    /// Exec infrastructure failed before or during execution.
    Failed,
}

/// Timing metadata for a completed exec. Timestamps are Unix ms (UTC).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecTiming {
    /// When the guest began spawning the process (Unix ms).
    pub spawned_at_unix_ms: u64,
    /// When the process exited or was killed (Unix ms).
    pub exited_at_unix_ms: u64,
    /// Time from request receipt to process spawn (ms).
    pub spawn_ms: u64,
    /// Time from process spawn to exit (ms).
    pub run_ms: u64,
}

/// Exec response payload — sent from guest to host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecResponse {
    /// Terminal status.
    pub status: ExecStatus,
    /// Process exit code when `status == Completed`. `None` when the process
    /// never produced an exit status.
    pub exit_code: Option<i32>,
    /// Standard output, inline.
    pub stdout: Vec<u8>,
    /// Standard error, inline.
    pub stderr: Vec<u8>,
    /// Whether stdout or stderr was truncated before being included in this
    /// response. `None` means not truncated or unknown.
    pub truncated: Option<bool>,
    /// Timing for this execution.
    pub timing: ExecTiming,
}

/// Shutdown request payload — host → guest. Carries optional context.
/// On receipt the guest should sync filesystems, send a [`ShutdownResponse`],
/// flush, and then exit (or invoke a system poweroff if not running as PID 1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShutdownRequest {
    /// Free-form reason logged on the guest. Optional.
    pub reason: Option<String>,
}

/// Shutdown response payload — guest → host. Acknowledgement that the
/// shutdown request was received and the guest is about to exit. Carries
/// the action the guest will take so the host can choose its post-stop
/// timeout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShutdownResponse {
    /// Action the guest will take after sending this ack.
    pub action: ShutdownAction,
}

/// What the guest will do after acknowledging a shutdown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShutdownAction {
    /// Guest will call `exit()`. When the guest is PID 1 (minimal image)
    /// this triggers a kernel panic; with `panic=1` in boot args the kernel
    /// reboots and Firecracker exits.
    Exit,
    /// Guest will exec `/sbin/poweroff -f` (ubuntu image; m80-guestd is a
    /// systemd service, not PID 1, so just exiting wouldn't shut the VM
    /// down).
    Poweroff,
}

/// Cancel request payload — host → guest. Asks the guest to kill the
/// in-flight exec identified by `request_id`.
///
/// Shared by the persistent-VM epic (`m80-qokt.2`) and the streaming-exec
/// epic (`m80-5vha`). Whichever lands first owns the type; the second
/// imports it without re-declaring.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CancelRequest {
    /// Must match the `request_id` on the `Envelope<ExecRequest>` that is
    /// being cancelled.
    pub request_id: String,
}

/// Cancel response — guest → host. Sent after the guest has
/// processed a [`CancelRequest`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CancelResponse {
    /// Echoed back from [`CancelRequest::request_id`].
    pub request_id: String,
    /// Outcome of the cancellation attempt.
    pub status: CancelStatus,
}

/// Outcome of a cancellation attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancelStatus {
    /// Guest killed the process on request (SIGKILL sent and reaped).
    Cancelled,
    /// Process had already exited before the cancel arrived (or the
    /// `request_id` did not match any in-flight exec).
    AlreadyExited,
    /// Guest could not kill the process (`kill(2)` itself failed — rare).
    Failed,
}

/// Reserved exact-version payload for tests and future protocol work.
///
/// Active application connections do not send this as a prelude; every
/// application [`Envelope`] carries and validates its own version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandshakeMessage {
    /// Protocol version this peer is running.
    pub version: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::io::Cursor;

    use crate::{read_frame, write_frame};

    fn sample_timing() -> ExecTiming {
        ExecTiming {
            spawned_at_unix_ms: 1_000_000,
            exited_at_unix_ms: 1_000_100,
            spawn_ms: 10,
            run_ms: 90,
        }
    }

    fn sample_request() -> ExecRequest {
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

    fn sample_response() -> ExecResponse {
        ExecResponse {
            status: ExecStatus::Completed,
            exit_code: Some(0),
            stdout: b"hello\n".to_vec(),
            stderr: Vec::new(),
            truncated: None,
            timing: sample_timing(),
        }
    }

    #[test]
    fn envelope_new_stamps_protocol_version_and_kind() {
        let env = Envelope::new(sample_request());
        assert_eq!(env.version, PROTOCOL_VERSION);
        assert_eq!(env.kind, PAYLOAD_KIND_EXEC_REQUEST);
        assert_eq!(env.request_id, None);

        let env = Envelope::new(sample_response());
        assert_eq!(env.kind, PAYLOAD_KIND_EXEC_RESPONSE);
    }

    #[test]
    fn envelope_with_request_id_stamps_kind() {
        let env = Envelope::with_request_id(sample_request(), "req-1".into());
        assert_eq!(env.version, PROTOCOL_VERSION);
        assert_eq!(env.kind, PAYLOAD_KIND_EXEC_REQUEST);
        assert_eq!(env.request_id, Some("req-1".to_owned()));
    }

    #[test]
    fn exec_status_variants_round_trip_in_protobuf() {
        for status in [
            ExecStatus::Completed,
            ExecStatus::TimedOut,
            ExecStatus::Cancelled,
            ExecStatus::Failed,
        ] {
            let mut response = sample_response();
            response.status = status;
            let env = Envelope::new(response);
            let mut buf = Vec::new();
            write_frame(&mut buf, &env).unwrap();
            let back: Envelope<ExecResponse> = read_frame(&mut Cursor::new(buf)).unwrap();
            assert_eq!(back.payload.status, status);
        }
    }

    #[test]
    fn handshake_message_round_trip() {
        let h = HandshakeMessage {
            version: PROTOCOL_VERSION,
        };
        let mut buf = Vec::new();
        write_frame(&mut buf, &h).unwrap();
        let back: HandshakeMessage = read_frame(&mut Cursor::new(buf)).unwrap();
        assert_eq!(back, h);
    }

    #[test]
    fn cancel_request_round_trip() {
        let req = CancelRequest {
            request_id: "req-42".into(),
        };
        let env = Envelope::with_request_id(req.clone(), "req-42".into());
        assert_eq!(env.kind, PAYLOAD_KIND_CANCEL_REQUEST);
        let mut buf = Vec::new();
        write_frame(&mut buf, &env).unwrap();
        let back: Envelope<CancelRequest> = read_frame(&mut Cursor::new(buf)).unwrap();
        assert_eq!(back.payload.request_id, "req-42");
        assert_eq!(back.kind, PAYLOAD_KIND_CANCEL_REQUEST);
    }

    #[test]
    fn cancel_ack_cancelled_round_trip() {
        let ack = CancelResponse {
            request_id: "req-42".into(),
            status: CancelStatus::Cancelled,
        };
        let env = Envelope::new(ack);
        assert_eq!(env.kind, PAYLOAD_KIND_CANCEL_RESPONSE);
        let mut buf = Vec::new();
        write_frame(&mut buf, &env).unwrap();
        let back: Envelope<CancelResponse> = read_frame(&mut Cursor::new(buf)).unwrap();
        assert_eq!(back.payload.status, CancelStatus::Cancelled);
    }

    #[test]
    fn cancel_ack_already_exited_round_trip() {
        let ack = CancelResponse {
            request_id: "req-7".into(),
            status: CancelStatus::AlreadyExited,
        };
        let env = Envelope::new(ack);
        let mut buf = Vec::new();
        write_frame(&mut buf, &env).unwrap();
        let back: Envelope<CancelResponse> = read_frame(&mut Cursor::new(buf)).unwrap();
        assert_eq!(back.payload.status, CancelStatus::AlreadyExited);
    }

    #[test]
    fn cancel_ack_failed_round_trip() {
        let ack = CancelResponse {
            request_id: "req-9".into(),
            status: CancelStatus::Failed,
        };
        let env = Envelope::new(ack);
        let mut buf = Vec::new();
        write_frame(&mut buf, &env).unwrap();
        let back: Envelope<CancelResponse> = read_frame(&mut Cursor::new(buf)).unwrap();
        assert_eq!(back.payload.status, CancelStatus::Failed);
    }
}
