use std::io::{BufReader, Read as _, Write as _};
use std::net::Shutdown;
use std::os::unix::net::UnixStream;

use serde::{Deserialize, Serialize};

use m80_firecracker::{ExecChunk, ExecExit, ExecRequest, ExecResponse, FcError};

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
        request: ExecRequest,
    },
    RunStream {
        profile: Option<String>,
        egress: String,
        request_id: String,
        request: ExecRequest,
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
    pub response: ExecResponse,
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
        exit: ExecExit,
        reset_decision: String,
        discard_reason: String,
        run_dir: String,
    },
    Error(WarmErrorResponse),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct WarmErrorResponse {
    pub variant: String,
    pub detail: String,
    pub request_id: Option<String>,
    pub target_ready: Option<usize>,
}

impl WarmErrorResponse {
    pub(super) fn from_error(err: &FcError) -> Self {
        Self::from_error_with_request_id(err, None)
    }

    pub(super) fn from_error_with_request_id(err: &FcError, request_id: Option<String>) -> Self {
        match err {
            FcError::PoolEmpty { target_ready } => Self {
                variant: "PoolEmpty".to_owned(),
                detail: err.to_string(),
                request_id,
                target_ready: Some(*target_ready),
            },
            FcError::Config(_) => Self {
                variant: "Config".to_owned(),
                detail: err.to_string(),
                request_id,
                target_ready: None,
            },
            _ => Self {
                variant: "Generic".to_owned(),
                detail: err.to_string(),
                request_id,
                target_ready: None,
            },
        }
    }

    pub(super) fn into_fc_error(self) -> FcError {
        match self.variant.as_str() {
            "PoolEmpty" => FcError::PoolEmpty {
                target_ready: self.target_ready.unwrap_or(1),
            },
            "Config" => FcError::Config(self.detail),
            _ => FcError::Config(format!("warm owner error: {}", self.detail)),
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

fn connect_owner() -> Result<UnixStream, FcError> {
    let socket = status::socket_path()?;
    UnixStream::connect(&socket).map_err(|e| {
        FcError::Config(format!(
            "warm owner unavailable at {}: {e}",
            socket.display()
        ))
    })
}

fn write_request(stream: &mut UnixStream, req: &WarmControlRequest) -> Result<(), FcError> {
    let payload = serde_json::to_vec(req)
        .map_err(|e| FcError::Config(format!("serialize warm control request: {e}")))?;
    stream.write_all(&payload).map_err(FcError::Io)?;
    stream.shutdown(Shutdown::Write).map_err(FcError::Io)
}

pub(super) fn read_request(stream: &mut UnixStream) -> Result<WarmControlRequest, FcError> {
    let mut bytes = Vec::new();
    stream.read_to_end(&mut bytes).map_err(FcError::Io)?;
    serde_json::from_slice(&bytes)
        .map_err(|e| FcError::Config(format!("parse warm control request: {e}")))
}

pub(super) fn write_response(
    stream: &mut UnixStream,
    response: &WarmControlResponse,
) -> Result<(), FcError> {
    let payload = serde_json::to_vec(response)
        .map_err(|e| FcError::Config(format!("serialize warm control response: {e}")))?;
    stream.write_all(&payload).map_err(FcError::Io)?;
    stream.flush().map_err(FcError::Io)
}

pub(super) fn write_stream_frame(
    stream: &mut UnixStream,
    frame: &WarmStreamFrame,
) -> Result<(), FcError> {
    let payload = serde_json::to_vec(frame)
        .map_err(|e| FcError::Config(format!("serialize warm stream frame: {e}")))?;
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
        return Err(FcError::Config(
            "warm owner closed stream before terminal frame".to_owned(),
        ));
    }
    serde_json::from_str(&line)
        .map_err(|e| FcError::Config(format!("parse warm stream frame: {e}")))
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
    serde_json::from_slice(&bytes)
        .map_err(|e| FcError::Config(format!("parse warm control response: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pool_empty_error_round_trips_to_fc_error() {
        let err = WarmErrorResponse::from_error(&FcError::PoolEmpty { target_ready: 2 });

        assert_eq!(err.variant, "PoolEmpty");
        assert_eq!(err.request_id, None);
        assert_eq!(err.target_ready, Some(2));
        assert!(matches!(
            err.into_fc_error(),
            FcError::PoolEmpty { target_ready: 2 }
        ));
    }

    #[test]
    fn config_error_round_trips_to_fc_error() {
        let err = WarmErrorResponse::from_error(&FcError::Config("bad".to_owned()));

        assert_eq!(err.variant, "Config");
        assert_eq!(err.request_id, None);
        assert!(matches!(err.into_fc_error(), FcError::Config(_)));
    }

    #[test]
    fn error_response_can_carry_request_id() {
        let err = WarmErrorResponse::from_error_with_request_id(
            &FcError::Config("bad".to_owned()),
            Some("req-warm".to_owned()),
        );

        assert_eq!(err.variant, "Config");
        assert_eq!(err.request_id.as_deref(), Some("req-warm"));
        assert!(matches!(err.into_fc_error(), FcError::Config(_)));
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
}
