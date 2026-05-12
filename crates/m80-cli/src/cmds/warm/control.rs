use std::fmt::Display;
use std::io::{BufReader, Read as _, Write as _};
use std::net::Shutdown;
use std::os::unix::net::UnixStream;

use serde::{Deserialize, Serialize};

use m80_firecracker::{ExecChunk, FcError, WireProtocolError};

use crate::cmds::proto_json::{ExecExitJson, ExecRequestJson, ExecResponseJson};
use crate::errors;

use super::status;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, tag = "type", rename_all = "snake_case")]
pub(super) enum WarmControlRequest {
    Status {
        profile: Option<String>,
    },
    Drain,
    Disable,
    Run {
        profile: Option<String>,
        egress: String,
        request_id: String,
        request: ExecRequestJson,
    },
    RunStream {
        profile: Option<String>,
        egress: String,
        request_id: String,
        request: ExecRequestJson,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, tag = "type", rename_all = "snake_case")]
pub(super) enum WarmControlResponse {
    Status(status::WarmStatus),
    Run(WarmRunResult),
    Error(WarmErrorResponse),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct WarmRunResult {
    pub request_id: String,
    pub response: ExecResponseJson,
    pub reset_decision: String,
    pub discard_reason: String,
    pub run_dir: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, tag = "type", rename_all = "snake_case")]
pub(super) enum WarmStreamFrame {
    Stdout {
        seq: u32,
        bytes: Vec<u8>,
    },
    Stderr {
        seq: u32,
        bytes: Vec<u8>,
    },
    Exit {
        request_id: String,
        exit: ExecExitJson,
        reset_decision: String,
        discard_reason: String,
        run_dir: String,
    },
    Error(WarmErrorResponse),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct WarmErrorResponse {
    pub variant: WarmErrorKind,
    pub detail: String,
    pub exit_code: i32,
    pub request_id: Option<String>,
    pub target_ready: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) enum WarmErrorKind {
    Preflight,
    Manifest,
    Storage,
    Jailer,
    Cgroup,
    Network,
    Client,
    Vsock,
    Snapshot,
    FileOp,
    DriveHotplug,
    TenantIdentityMismatch,
    AdmissionRefused,
    PoolEmpty,
    InvalidState,
    ApiSocketTimeout,
    GuestdReadyTimeout,
    RunDirOwnershipAmbiguous,
    RunDirAlreadyOwned,
    RunDirNotFound,
    PathIo,
    Json,
    UnsupportedOperation,
    CommandSpawnFailed,
    CommandFailed,
    ArtifactMissing,
    WarmPoolFillFailed,
    WarmReadyProbeRejected,
    WarmReadyProbeNoResult,
    WarmOwnerSocketExists,
    WarmOwnerNotAcceptingLeases,
    WarmOwnerDrainTimeout,
    WarmCompatibilityMismatch,
    UnexpectedWarmResponse,
    KillFailed,
    ReapTimeout,
    ReapFailed,
    Io,
    Config,
    IdleTimedOut,
    OneShotConsumed,
    Protocol,
}

impl WarmErrorKind {
    fn from_error(err: &FcError) -> Self {
        match err {
            FcError::Preflight(_) => Self::Preflight,
            FcError::Manifest(_) => Self::Manifest,
            FcError::Storage(_) => Self::Storage,
            FcError::Jailer(_) => Self::Jailer,
            FcError::Cgroup(_) => Self::Cgroup,
            FcError::Network(_) => Self::Network,
            FcError::Client(_) => Self::Client,
            FcError::Vsock(_) => Self::Vsock,
            FcError::Protocol(_) => Self::Protocol,
            FcError::Snapshot(_) => Self::Snapshot,
            FcError::FileOp(_) => Self::FileOp,
            FcError::DriveHotplug(_) => Self::DriveHotplug,
            FcError::TenantIdentityMismatch { .. } => Self::TenantIdentityMismatch,
            FcError::AdmissionRefused { .. } => Self::AdmissionRefused,
            FcError::PoolEmpty { .. } => Self::PoolEmpty,
            FcError::InvalidState { .. } => Self::InvalidState,
            FcError::ApiSocketTimeout { .. } => Self::ApiSocketTimeout,
            FcError::GuestdReadyTimeout { .. } => Self::GuestdReadyTimeout,
            FcError::RunDirOwnershipAmbiguous { .. } => Self::RunDirOwnershipAmbiguous,
            FcError::RunDirAlreadyOwned { .. } => Self::RunDirAlreadyOwned,
            FcError::RunDirNotFound { .. } => Self::RunDirNotFound,
            FcError::PathIo { .. } => Self::PathIo,
            FcError::Json { .. } => Self::Json,
            FcError::UnsupportedOperation { .. } => Self::UnsupportedOperation,
            FcError::CommandSpawnFailed { .. } => Self::CommandSpawnFailed,
            FcError::CommandFailed { .. } => Self::CommandFailed,
            FcError::ArtifactMissing { .. } => Self::ArtifactMissing,
            FcError::WarmPoolFillFailed { .. } => Self::WarmPoolFillFailed,
            FcError::WarmReadyProbeRejected { .. } => Self::WarmReadyProbeRejected,
            FcError::WarmReadyProbeNoResult => Self::WarmReadyProbeNoResult,
            FcError::WarmOwnerSocketExists { .. } => Self::WarmOwnerSocketExists,
            FcError::WarmOwnerNotAcceptingLeases => Self::WarmOwnerNotAcceptingLeases,
            FcError::WarmOwnerDrainTimeout { .. } => Self::WarmOwnerDrainTimeout,
            FcError::WarmCompatibilityMismatch { .. } => Self::WarmCompatibilityMismatch,
            FcError::UnexpectedWarmResponse { .. } => Self::UnexpectedWarmResponse,
            FcError::KillFailed { .. } => Self::KillFailed,
            FcError::ReapTimeout { .. } => Self::ReapTimeout,
            FcError::ReapFailed { .. } => Self::ReapFailed,
            FcError::Io(_) => Self::Io,
            FcError::Config(_) => Self::Config,
            FcError::IdleTimedOut => Self::IdleTimedOut,
            FcError::OneShotConsumed => Self::OneShotConsumed,
        }
    }

    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Preflight => "Preflight",
            Self::Manifest => "Manifest",
            Self::Storage => "Storage",
            Self::Jailer => "Jailer",
            Self::Cgroup => "Cgroup",
            Self::Network => "Network",
            Self::Client => "Client",
            Self::Vsock => "Vsock",
            Self::Snapshot => "Snapshot",
            Self::FileOp => "FileOp",
            Self::DriveHotplug => "DriveHotplug",
            Self::TenantIdentityMismatch => "TenantIdentityMismatch",
            Self::AdmissionRefused => "AdmissionRefused",
            Self::PoolEmpty => "PoolEmpty",
            Self::InvalidState => "InvalidState",
            Self::ApiSocketTimeout => "ApiSocketTimeout",
            Self::GuestdReadyTimeout => "GuestdReadyTimeout",
            Self::RunDirOwnershipAmbiguous => "RunDirOwnershipAmbiguous",
            Self::RunDirAlreadyOwned => "RunDirAlreadyOwned",
            Self::RunDirNotFound => "RunDirNotFound",
            Self::PathIo => "PathIo",
            Self::Json => "Json",
            Self::UnsupportedOperation => "UnsupportedOperation",
            Self::CommandSpawnFailed => "CommandSpawnFailed",
            Self::CommandFailed => "CommandFailed",
            Self::ArtifactMissing => "ArtifactMissing",
            Self::WarmPoolFillFailed => "WarmPoolFillFailed",
            Self::WarmReadyProbeRejected => "WarmReadyProbeRejected",
            Self::WarmReadyProbeNoResult => "WarmReadyProbeNoResult",
            Self::WarmOwnerSocketExists => "WarmOwnerSocketExists",
            Self::WarmOwnerNotAcceptingLeases => "WarmOwnerNotAcceptingLeases",
            Self::WarmOwnerDrainTimeout => "WarmOwnerDrainTimeout",
            Self::WarmCompatibilityMismatch => "WarmCompatibilityMismatch",
            Self::UnexpectedWarmResponse => "UnexpectedWarmResponse",
            Self::KillFailed => "KillFailed",
            Self::ReapTimeout => "ReapTimeout",
            Self::ReapFailed => "ReapFailed",
            Self::Io => "Io",
            Self::Config => "Config",
            Self::IdleTimedOut => "IdleTimedOut",
            Self::OneShotConsumed => "OneShotConsumed",
            Self::Protocol => "Protocol",
        }
    }
}

impl WarmErrorResponse {
    pub(super) fn from_error(err: &FcError) -> Self {
        Self::from_error_with_request_id(err, None)
    }

    pub(super) fn from_error_with_request_id(err: &FcError, request_id: Option<String>) -> Self {
        let target_ready = match err {
            FcError::PoolEmpty { target_ready } => Some(*target_ready),
            _ => None,
        };
        Self {
            variant: WarmErrorKind::from_error(err),
            detail: err.to_string(),
            exit_code: errors::exit_code_for(err),
            request_id,
            target_ready,
        }
    }
}

pub(super) fn send_to_owner(req: WarmControlRequest) -> Result<WarmControlResponse, FcError> {
    let mut stream = connect_owner()?;
    write_request(&mut stream, &req)?;
    read_response(&mut stream)
}

pub(super) fn send_stream_request(
    req: WarmControlRequest,
) -> Result<BufReader<UnixStream>, FcError> {
    let mut stream = connect_owner()?;
    write_request(&mut stream, &req)?;
    Ok(BufReader::new(stream))
}

fn malformed_peer(context: &str, source: impl Display) -> FcError {
    FcError::Protocol(WireProtocolError::MalformedPeer(format!(
        "{context}: {source}"
    )))
}

fn connect_owner() -> Result<UnixStream, FcError> {
    let socket = status::socket_path()?;
    UnixStream::connect(&socket).map_err(|e| {
        FcError::Io(std::io::Error::new(
            e.kind(),
            format!("warm owner unavailable at {}: {e}", socket.display()),
        ))
    })
}

fn write_request(stream: &mut UnixStream, req: &WarmControlRequest) -> Result<(), FcError> {
    let payload =
        serde_json::to_vec(req).map_err(|e| malformed_peer("serialize warm control request", e))?;
    stream.write_all(&payload).map_err(FcError::Io)?;
    stream.shutdown(Shutdown::Write).map_err(FcError::Io)
}

pub(super) fn read_request(stream: &mut UnixStream) -> Result<WarmControlRequest, FcError> {
    let mut bytes = Vec::new();
    stream.read_to_end(&mut bytes).map_err(FcError::Io)?;
    serde_json::from_slice(&bytes).map_err(|e| malformed_peer("parse warm control request", e))
}

pub(super) fn write_response(
    stream: &mut UnixStream,
    response: &WarmControlResponse,
) -> Result<(), FcError> {
    let payload = serde_json::to_vec(response)
        .map_err(|e| malformed_peer("serialize warm control response", e))?;
    stream.write_all(&payload).map_err(FcError::Io)?;
    stream.flush().map_err(FcError::Io)
}

pub(super) fn write_stream_frame(
    stream: &mut UnixStream,
    frame: &WarmStreamFrame,
) -> Result<(), FcError> {
    let payload =
        serde_json::to_vec(frame).map_err(|e| malformed_peer("serialize warm stream frame", e))?;
    stream.write_all(&payload).map_err(FcError::Io)?;
    stream.write_all(b"\n").map_err(FcError::Io)?;
    stream.flush().map_err(FcError::Io)
}

pub(super) fn read_stream_frame<R>(reader: &mut R) -> Result<WarmStreamFrame, FcError>
where
    R: std::io::BufRead,
{
    let mut line = String::new();
    let read = reader.read_line(&mut line).map_err(FcError::Io)?;
    if read == 0 {
        return Err(FcError::Protocol(
            WireProtocolError::DisconnectBeforeTerminal {
                context: "warm stream",
            },
        ));
    }
    serde_json::from_str(&line).map_err(|e| malformed_peer("parse warm stream frame", e))
}

pub(super) fn stream_frame_for_chunk(chunk: ExecChunk) -> WarmStreamFrame {
    match chunk {
        ExecChunk::Stdout { seq, bytes } => WarmStreamFrame::Stdout { seq, bytes },
        ExecChunk::Stderr { seq, bytes } => WarmStreamFrame::Stderr { seq, bytes },
    }
}

fn read_response(stream: &mut UnixStream) -> Result<WarmControlResponse, FcError> {
    let mut bytes = Vec::new();
    stream.read_to_end(&mut bytes).map_err(FcError::Io)?;
    serde_json::from_slice(&bytes).map_err(|e| malformed_peer("parse warm control response", e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use m80_firecracker::ConfigError;

    #[test]
    fn pool_empty_error_preserves_variant_and_exit_code() {
        let err = WarmErrorResponse::from_error(&FcError::PoolEmpty { target_ready: 2 });

        assert_eq!(err.variant, WarmErrorKind::PoolEmpty);
        assert_eq!(err.exit_code, errors::EXIT_POOL_EMPTY);
        assert_eq!(err.request_id, None);
        assert_eq!(err.target_ready, Some(2));
    }

    #[test]
    fn config_error_preserves_variant_and_exit_code() {
        let err = WarmErrorResponse::from_error(&FcError::Config(ConfigError::InvalidValue {
            field: "config",
            reason: "bad".into(),
        }));

        assert_eq!(err.variant, WarmErrorKind::Config);
        assert_eq!(err.exit_code, errors::EXIT_CONFIG);
        assert_eq!(err.request_id, None);
        assert_eq!(err.target_ready, None);
    }

    #[test]
    fn preflight_owner_error_does_not_collapse_to_generic() {
        let err = WarmErrorResponse::from_error(&FcError::Preflight(
            m80_preflight::PreflightError::KvmUnavailable {
                path: "/dev/kvm".into(),
            },
        ));

        assert_eq!(err.variant, WarmErrorKind::Preflight);
        assert_eq!(err.variant.as_str(), "Preflight");
        assert_eq!(err.exit_code, errors::EXIT_PREFLIGHT);
        assert_eq!(err.target_ready, None);
    }

    #[test]
    fn error_response_can_carry_request_id() {
        let err = WarmErrorResponse::from_error_with_request_id(
            &FcError::Config(ConfigError::InvalidValue {
                field: "config",
                reason: "bad".into(),
            }),
            Some("req-warm".to_owned()),
        );

        assert_eq!(err.variant, WarmErrorKind::Config);
        assert_eq!(err.request_id.as_deref(), Some("req-warm"));
    }

    #[test]
    fn stream_frame_round_trips_as_newline_json() {
        let mut bytes = Vec::new();
        let frame = WarmStreamFrame::Stdout {
            seq: 7,
            bytes: b"abc".to_vec(),
        };

        serde_json::to_writer(&mut bytes, &frame).expect("serialize frame");
        bytes.push(b'\n');
        let mut reader = BufReader::new(bytes.as_slice());
        let parsed = read_stream_frame(&mut reader).expect("read frame");

        match parsed {
            WarmStreamFrame::Stdout { seq, bytes } => {
                assert_eq!(seq, 7);
                assert_eq!(bytes, b"abc");
            }
            other => panic!("expected stdout frame, got {other:?}"),
        }
    }

    #[test]
    fn malformed_stream_frame_is_protocol_not_config() {
        let mut reader = BufReader::new(b"{bad json}\n".as_slice());

        let err = read_stream_frame(&mut reader).expect_err("malformed frame must fail");

        assert!(matches!(
            err,
            FcError::Protocol(WireProtocolError::MalformedPeer(_))
        ));
        let envelope = WarmErrorResponse::from_error(&err);
        assert_eq!(envelope.variant, WarmErrorKind::Protocol);
        assert_eq!(envelope.exit_code, errors::EXIT_GENERIC);
    }

    #[test]
    fn closed_stream_frame_is_protocol_disconnect() {
        let mut reader = BufReader::new(b"".as_slice());

        let err = read_stream_frame(&mut reader).expect_err("empty stream must fail");

        assert!(matches!(
            err,
            FcError::Protocol(WireProtocolError::DisconnectBeforeTerminal {
                context: "warm stream"
            })
        ));
    }
}
