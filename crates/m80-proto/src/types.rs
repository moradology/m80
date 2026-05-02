//! Wire types: envelope, exec request/response, status, timing, handshake.

use serde::{Deserialize, Serialize};

use crate::version::PROTOCOL_VERSION;

/// Wire `kind` value for an envelope carrying [`ExecRequest`].
pub const PAYLOAD_KIND_EXEC_REQUEST: &str = "exec_request";

/// Wire `kind` value for an envelope carrying [`ExecResponse`].
pub const PAYLOAD_KIND_EXEC_RESPONSE: &str = "exec_response";

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
    /// Path inside the guest where the caller's workspace has been mounted.
    /// `None` → no workspace is attached.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_dir: Option<String>,
    /// Wall-clock budget in milliseconds before the guest kills the process.
    /// `None` → the guest applies its own default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
}

/// Terminal status of an exec operation.
///
/// `#[non_exhaustive]` so adding variants in a future revision (e.g.,
/// `KilledBySignal`) is a non-breaking change for downstream `match`
/// expressions. Wire serialization is `snake_case` of the variant name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ExecStatus {
    /// Process exited; see `exit_code` for the code.
    Completed,
    /// Guest killed the process because it exceeded `timeout_ms`.
    TimedOut,
    /// Caller requested cancellation.
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
    /// Reserved for the v0.2 externalization story. Always `None` in v0.1.
    /// Skipped on serialization when `None` so v0.1 wire bytes are unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub truncated: Option<bool>,
    /// Timing for this execution.
    pub timing: ExecTiming,
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

mod b64 {
    pub mod single {
        use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
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
        use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
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

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_request() -> ExecRequest {
        ExecRequest {
            program: "/bin/sh".into(),
            args: vec!["-c".into(), "echo hi".into()],
            cwd: None,
            env: None,
            stdin: None,
            workspace_dir: None,
            timeout_ms: Some(5_000),
        }
    }

    fn sample_response() -> ExecResponse {
        ExecResponse {
            status: ExecStatus::Completed,
            exit_code: Some(0),
            stdout: b"hello\n".to_vec(),
            stderr: Vec::new(),
            truncated: None,
            timing: ExecTiming {
                spawned_at_unix_ms: 1_000_000,
                exited_at_unix_ms: 1_000_100,
                spawn_ms: 10,
                run_ms: 90,
            },
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
        let h = HandshakeMessage { version: PROTOCOL_VERSION };
        let json = serde_json::to_string(&h).unwrap();
        let back: HandshakeMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(back, h);
    }
}
