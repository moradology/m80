//! Wire types: envelope, exec request/response, status, timing, handshake.

mod fileops;
mod metrics;
mod pty;
mod streaming;

pub use fileops::{
    DirEntry, FileError, FileKind, FileListRequest, FileListResponse, FileReadRequest,
    FileReadResponse, FileRemoveRequest, FileRemoveResponse, FileStat, FileStatRequest,
    FileStatResponse, FileWriteBeginRequest, FileWriteBeginResponse, FileWriteChunkRequest,
    FileWriteChunkResponse, FileWriteCommitRequest, FileWriteCommitResponse, FileWriteRequest,
    FileWriteResponse, FILE_READ_LIMIT_DEFAULT, PAYLOAD_KIND_FILE_LIST_REQUEST,
    PAYLOAD_KIND_FILE_LIST_RESPONSE, PAYLOAD_KIND_FILE_READ_REQUEST,
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

use serde::{Deserialize, Serialize};

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

/// Wire `kind` value for an envelope carrying [`CancelAck`].
pub const PAYLOAD_KIND_CANCEL_ACK: &str = "cancel_ack";

/// Marker trait for types that have a canonical wire `kind`.
///
/// Implemented for [`ExecRequest`] and [`ExecResponse`] in v0.1; future
/// payload types add their own impls without bumping `PROTOCOL_VERSION`.
pub trait Payload {
    /// Wire `kind` value, stamped into [`Envelope::kind`] by the constructors.
    const KIND: &'static str;
}

impl Payload for ExecRequest {
    const KIND: &'static str = PAYLOAD_KIND_EXEC_REQUEST;
}

impl Payload for ExecResponse {
    const KIND: &'static str = PAYLOAD_KIND_EXEC_RESPONSE;
}

impl Payload for ShutdownRequest {
    const KIND: &'static str = PAYLOAD_KIND_SHUTDOWN_REQUEST;
}

impl Payload for ShutdownResponse {
    const KIND: &'static str = PAYLOAD_KIND_SHUTDOWN_RESPONSE;
}

impl Payload for CancelRequest {
    const KIND: &'static str = PAYLOAD_KIND_CANCEL_REQUEST;
}

impl Payload for CancelAck {
    const KIND: &'static str = PAYLOAD_KIND_CANCEL_ACK;
}

/// Wire envelope wrapping an opaque payload.
///
/// `version` fails closed at the framing layer. `kind` is reserved for
/// forward-compatible payload-type extension. `request_id` is opaque to the
/// protocol; consumers attach meaning.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Envelope<T> {
    /// Protocol version. Must equal `PROTOCOL_VERSION` on every frame.
    pub version: u32,
    /// Payload-type discriminator. Stamped by [`Envelope::new`] /
    /// [`Envelope::with_request_id`] using `T::KIND`.
    pub kind: String,
    /// Optional caller-supplied identifier echoed back in the response.
    #[serde(default, skip_serializing_if = "Option::is_none")]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecRequest {
    /// Executable path or name to spawn.
    pub program: String,
    /// Arguments passed to `program` (not including `program` itself).
    pub args: Vec<String>,
    /// Working directory for the spawned process. `None` → inherit from guest.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// Additional environment variables as `(key, value)` pairs.
    /// `None` → inherit the guest's environment unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env: Option<Vec<(String, String)>>,
    /// Bytes to feed to the process on stdin. `None` → stdin is closed.
    /// On the wire: base64-encoded JSON string when present.
    #[serde(default, skip_serializing_if = "Option::is_none", with = "b64::opt")]
    pub stdin: Option<Vec<u8>>,
    /// Wall-clock budget in milliseconds before the guest kills the process.
    /// `None` → the guest applies its own default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
    /// Opt into multi-frame stdout/stderr streaming. `false` preserves the
    /// buffered v0.1 response shape and is skipped on the wire.
    #[serde(default, skip_serializing_if = "is_false")]
    pub streaming: bool,
}

/// Terminal status of an exec operation. Wire serialization is the
/// `snake_case` variant name. Adding a variant requires a
/// `PROTOCOL_VERSION` bump.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecResponse {
    /// Terminal status.
    pub status: ExecStatus,
    /// Process exit code when `status == Completed`. `None` when the process
    /// never produced an exit status.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    /// Standard output, inline. On the wire: base64-encoded JSON string.
    #[serde(with = "b64::single")]
    pub stdout: Vec<u8>,
    /// Standard error, inline. On the wire: base64-encoded JSON string.
    #[serde(with = "b64::single")]
    pub stderr: Vec<u8>,
    /// Whether stdout or stderr was truncated before being included in this
    /// response. `None` means not truncated or unknown. Skipped on
    /// serialization when `None` so wire bytes are unchanged when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub truncated: Option<bool>,
    /// Timing for this execution.
    pub timing: ExecTiming,
}

/// Shutdown request payload — host → guest. Carries optional context.
/// On receipt the guest should sync filesystems, send a [`ShutdownResponse`],
/// flush, and then exit (or invoke a system poweroff if not running as PID 1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShutdownRequest {
    /// Free-form reason logged on the guest. Optional.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Shutdown response payload — guest → host. Acknowledgement that the
/// shutdown request was received and the guest is about to exit. Carries
/// the action the guest will take so the host can choose its post-stop
/// timeout.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShutdownResponse {
    /// Action the guest will take after sending this ack.
    pub action: ShutdownAction,
}

/// What the guest will do after acknowledging a shutdown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CancelRequest {
    /// Must match the `request_id` on the `Envelope<ExecRequest>` that is
    /// being cancelled.
    pub request_id: String,
}

/// Cancel acknowledgement — guest → host. Sent after the guest has
/// processed a [`CancelRequest`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CancelAck {
    /// Echoed back from [`CancelRequest::request_id`].
    pub request_id: String,
    /// Outcome of the cancellation attempt.
    pub status: CancelStatus,
}

/// Outcome of a cancellation attempt. Wire serialization is `snake_case`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub enum CancelStatus {
    /// Guest killed the process on request (SIGKILL sent and reaped).
    Cancelled,
    /// Process had already exited before the cancel arrived (or the
    /// `request_id` did not match any in-flight exec).
    AlreadyExited,
    /// Guest could not kill the process (`kill(2)` itself failed — rare).
    Failed,
}

/// Version-exchange message sent on every fresh connection before any
/// application payload. Only after a successful [`crate::negotiate_version`]
/// should either side send an [`Envelope`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandshakeMessage {
    /// Protocol version this peer is running.
    pub version: u32,
}

pub(crate) mod b64 {
    pub mod single {
        use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
        use serde::{Deserialize, Deserializer, Serializer};

        pub fn serialize<S: Serializer>(v: &Vec<u8>, s: S) -> Result<S::Ok, S::Error> {
            s.serialize_str(&B64.encode(v))
        }

        pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
            let encoded = String::deserialize(d)?;
            B64.decode(encoded).map_err(serde::de::Error::custom)
        }
    }

    pub mod opt {
        use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
        use serde::{Deserialize, Deserializer, Serializer};

        pub fn serialize<S: Serializer>(v: &Option<Vec<u8>>, s: S) -> Result<S::Ok, S::Error> {
            match v {
                None => s.serialize_none(),
                Some(bytes) => s.serialize_str(&B64.encode(bytes)),
            }
        }

        pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Vec<u8>>, D::Error> {
            let opt = Option::<String>::deserialize(d)?;
            match opt {
                None => Ok(None),
                Some(s) => B64.decode(s).map(Some).map_err(serde::de::Error::custom),
            }
        }
    }
}

fn is_false(v: &bool) -> bool {
    !*v
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::test_helpers::{sample_request, sample_response};

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
    fn exec_status_serde_variants() {
        for (status, expected_json) in [
            (ExecStatus::Completed, "\"completed\""),
            (ExecStatus::TimedOut, "\"timed_out\""),
            (ExecStatus::Cancelled, "\"cancelled\""),
            (ExecStatus::Failed, "\"failed\""),
        ] {
            let json = serde_json::to_string(&status).unwrap();
            assert_eq!(json, expected_json);
            let back: ExecStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(back, status);
        }
    }

    #[test]
    fn handshake_message_round_trip() {
        let h = HandshakeMessage {
            version: PROTOCOL_VERSION,
        };
        let json = serde_json::to_string(&h).unwrap();
        let back: HandshakeMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(back, h);
    }

    #[test]
    fn cancel_request_round_trip() {
        let req = CancelRequest {
            request_id: "req-42".into(),
        };
        let env = Envelope::with_request_id(req.clone(), "req-42".into());
        assert_eq!(env.kind, PAYLOAD_KIND_CANCEL_REQUEST);
        let json = serde_json::to_string(&env).unwrap();
        let back: Envelope<CancelRequest> = serde_json::from_str(&json).unwrap();
        assert_eq!(back.payload.request_id, "req-42");
        assert_eq!(back.kind, PAYLOAD_KIND_CANCEL_REQUEST);
    }

    #[test]
    fn cancel_ack_cancelled_round_trip() {
        let ack = CancelAck {
            request_id: "req-42".into(),
            status: CancelStatus::Cancelled,
        };
        let env = Envelope::new(ack);
        assert_eq!(env.kind, PAYLOAD_KIND_CANCEL_ACK);
        let json = serde_json::to_string(&env).unwrap();
        let back: Envelope<CancelAck> = serde_json::from_str(&json).unwrap();
        assert_eq!(back.payload.status, CancelStatus::Cancelled);
    }

    #[test]
    fn cancel_ack_already_exited_round_trip() {
        let ack = CancelAck {
            request_id: "req-7".into(),
            status: CancelStatus::AlreadyExited,
        };
        let json = serde_json::to_string(&ack).unwrap();
        let back: CancelAck = serde_json::from_str(&json).unwrap();
        assert_eq!(back.status, CancelStatus::AlreadyExited);
    }

    #[test]
    fn cancel_ack_failed_round_trip() {
        let ack = CancelAck {
            request_id: "req-9".into(),
            status: CancelStatus::Failed,
        };
        let json = serde_json::to_string(&ack).unwrap();
        let back: CancelAck = serde_json::from_str(&json).unwrap();
        assert_eq!(back.status, CancelStatus::Failed);
    }

    #[test]
    fn cancel_status_wire_values() {
        for (status, expected) in [
            (CancelStatus::Cancelled, "\"cancelled\""),
            (CancelStatus::AlreadyExited, "\"already_exited\""),
            (CancelStatus::Failed, "\"failed\""),
        ] {
            let json = serde_json::to_string(&status).unwrap();
            assert_eq!(json, expected);
            let back: CancelStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(back, status);
        }
    }
}
