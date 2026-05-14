//! Finite request/response protocol for the privileged outbound-network helper.

use std::io::{BufRead, Write};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::{
    apply_outbound_nat_policy, cleanup_orphan_bridge, cleanup_vm, read_vm_network_state_record,
    realize_bridge_and_tap, NetError, OutboundIntent, RealizedNetwork,
};

/// Maximum JSON request or response frame accepted by the network helper.
pub const NETWORK_HELPER_MAX_FRAME_BYTES: usize = 1024 * 1024;

/// One privileged network operation the helper may perform.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum NetworkHelperRequest {
    /// Realize the run-root bridge plus one VM's veth/private-netns/TAP topology.
    RealizeBridgeAndTap {
        /// Pre-validated outbound intent from `m80-net-mode`.
        intent: OutboundIntent,
        /// VM id whose deterministic names are being realized.
        vm_id: String,
        /// Backend run-root.
        run_root: PathBuf,
        /// Per-VM run directory.
        run_dir: PathBuf,
    },
    /// Apply host sysctl and iptables policy for one ready VM state.
    ApplyOutboundNatPolicy {
        /// Per-VM run directory containing `network-state.json`.
        run_dir: PathBuf,
    },
    /// Clean one VM's owned network topology and iptables policy.
    CleanupVm {
        /// VM id to clean.
        vm_id: String,
        /// Backend run-root.
        run_root: PathBuf,
    },
    /// Clean the orphan run-root bridge when no VM state remains.
    CleanupOrphanBridge {
        /// Backend run-root.
        run_root: PathBuf,
    },
}

/// Successful helper operation payload.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum NetworkHelperSuccess {
    /// Operation completed and has no structured return payload.
    Empty,
    /// Outbound topology realization result.
    RealizedNetwork {
        /// Caller-observable realized network handles.
        realized: RealizedNetwork,
    },
}

/// Helper response frame.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum NetworkHelperResponse {
    /// Operation succeeded.
    Ok {
        /// Successful operation payload.
        success: NetworkHelperSuccess,
    },
    /// Operation failed.
    Err {
        /// Typed helper failure.
        failure: NetworkHelperFailure,
    },
}

/// Typed helper failure categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub enum NetworkHelperFailureKind {
    /// The request frame was malformed, unknown, or too large.
    InvalidRequest,
    /// A known operation failed while mutating host network state.
    OperationFailed,
}

/// Helper failure payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetworkHelperFailure {
    /// Machine-readable failure class.
    pub kind: NetworkHelperFailureKind,
    /// Human-readable diagnostic detail.
    pub detail: String,
}

/// Decode and validate one request frame.
pub fn decode_network_helper_request(
    frame: &[u8],
) -> Result<NetworkHelperRequest, NetworkHelperFailure> {
    if frame.len() > NETWORK_HELPER_MAX_FRAME_BYTES {
        return Err(NetworkHelperFailure {
            kind: NetworkHelperFailureKind::InvalidRequest,
            detail: format!(
                "network helper request exceeds {} bytes",
                NETWORK_HELPER_MAX_FRAME_BYTES
            ),
        });
    }
    serde_json::from_slice(frame).map_err(|source| NetworkHelperFailure {
        kind: NetworkHelperFailureKind::InvalidRequest,
        detail: source.to_string(),
    })
}

/// Encode one helper response frame.
pub fn encode_network_helper_response(
    response: &NetworkHelperResponse,
) -> Result<Vec<u8>, NetworkHelperFailure> {
    let mut frame = serde_json::to_vec(response).map_err(|source| NetworkHelperFailure {
        kind: NetworkHelperFailureKind::OperationFailed,
        detail: source.to_string(),
    })?;
    if frame.len() > NETWORK_HELPER_MAX_FRAME_BYTES {
        return Err(NetworkHelperFailure {
            kind: NetworkHelperFailureKind::OperationFailed,
            detail: format!(
                "network helper response exceeds {} bytes",
                NETWORK_HELPER_MAX_FRAME_BYTES
            ),
        });
    }
    frame.push(b'\n');
    Ok(frame)
}

/// Execute one decoded helper request.
pub(crate) fn execute_network_helper_request(
    request: NetworkHelperRequest,
) -> NetworkHelperResponse {
    match execute_network_helper_request_inner(request) {
        Ok(success) => NetworkHelperResponse::Ok { success },
        Err(source) => NetworkHelperResponse::Err {
            failure: NetworkHelperFailure {
                kind: NetworkHelperFailureKind::OperationFailed,
                detail: source.to_string(),
            },
        },
    }
}

fn execute_network_helper_request_inner(
    request: NetworkHelperRequest,
) -> Result<NetworkHelperSuccess, NetError> {
    match request {
        NetworkHelperRequest::RealizeBridgeAndTap {
            intent,
            vm_id,
            run_root,
            run_dir,
        } => {
            let realized = realize_bridge_and_tap(&intent, &vm_id, &run_root, &run_dir)?;
            Ok(NetworkHelperSuccess::RealizedNetwork { realized })
        }
        NetworkHelperRequest::ApplyOutboundNatPolicy { run_dir } => {
            let state = read_vm_network_state_record(&run_dir)?;
            apply_outbound_nat_policy(&state)?;
            Ok(NetworkHelperSuccess::Empty)
        }
        NetworkHelperRequest::CleanupVm { vm_id, run_root } => {
            cleanup_vm(&vm_id, &run_root)?;
            Ok(NetworkHelperSuccess::Empty)
        }
        NetworkHelperRequest::CleanupOrphanBridge { run_root } => {
            cleanup_orphan_bridge(&run_root)?;
            Ok(NetworkHelperSuccess::Empty)
        }
    }
}

/// Serve newline-delimited JSON helper requests on stdio-like streams.
pub fn serve_network_helper_stdio<R, W>(mut reader: R, mut writer: W) -> std::io::Result<()>
where
    R: BufRead,
    W: Write,
{
    let mut frame = Vec::new();
    loop {
        let response = match read_next_network_helper_frame(&mut reader, &mut frame)? {
            NetworkHelperFrameRead::Eof => return Ok(()),
            NetworkHelperFrameRead::Frame => match decode_network_helper_request(&frame) {
                Ok(request) => execute_network_helper_request(request),
                Err(failure) => NetworkHelperResponse::Err { failure },
            },
            NetworkHelperFrameRead::Oversized => NetworkHelperResponse::Err {
                failure: NetworkHelperFailure {
                    kind: NetworkHelperFailureKind::InvalidRequest,
                    detail: format!(
                        "network helper request exceeds {} bytes",
                        NETWORK_HELPER_MAX_FRAME_BYTES
                    ),
                },
            },
        };
        let encoded = encode_network_helper_response(&response)
            .map_err(|failure| std::io::Error::other(failure.detail))?;
        writer.write_all(&encoded)?;
        writer.flush()?;
    }
}

enum NetworkHelperFrameRead {
    Eof,
    Frame,
    Oversized,
}

fn read_next_network_helper_frame<R>(
    reader: &mut R,
    frame: &mut Vec<u8>,
) -> std::io::Result<NetworkHelperFrameRead>
where
    R: BufRead,
{
    frame.clear();
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            return if frame.is_empty() {
                Ok(NetworkHelperFrameRead::Eof)
            } else {
                trim_frame_cr(frame);
                Ok(NetworkHelperFrameRead::Frame)
            };
        }
        if let Some(newline) = available.iter().position(|byte| *byte == b'\n') {
            if frame.len() + newline > NETWORK_HELPER_MAX_FRAME_BYTES {
                reader.consume(newline + 1);
                frame.clear();
                return Ok(NetworkHelperFrameRead::Oversized);
            }
            frame.extend_from_slice(&available[..newline]);
            reader.consume(newline + 1);
            trim_frame_cr(frame);
            return Ok(NetworkHelperFrameRead::Frame);
        }
        let available_len = available.len();
        if frame.len() + available_len > NETWORK_HELPER_MAX_FRAME_BYTES {
            reader.consume(available_len);
            discard_until_newline(reader)?;
            frame.clear();
            return Ok(NetworkHelperFrameRead::Oversized);
        }
        frame.extend_from_slice(available);
        reader.consume(available_len);
    }
}

fn discard_until_newline<R>(reader: &mut R) -> std::io::Result<()>
where
    R: BufRead,
{
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            return Ok(());
        }
        if let Some(newline) = available.iter().position(|byte| *byte == b'\n') {
            reader.consume(newline + 1);
            return Ok(());
        }
        let available_len = available.len();
        reader.consume(available_len);
    }
}

fn trim_frame_cr(frame: &mut Vec<u8>) {
    while matches!(frame.last(), Some(b'\r')) {
        frame.pop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_operation_is_rejected() {
        let err = decode_network_helper_request(br#"{"op":"arbitrary_shell","argv":["ip"]}"#)
            .unwrap_err();

        assert_eq!(err.kind, NetworkHelperFailureKind::InvalidRequest);
    }

    #[test]
    fn oversized_request_is_rejected() {
        let frame = vec![b' '; NETWORK_HELPER_MAX_FRAME_BYTES + 1];

        let err = decode_network_helper_request(&frame).unwrap_err();

        assert_eq!(err.kind, NetworkHelperFailureKind::InvalidRequest);
        assert!(err.detail.contains("exceeds"));
    }

    #[test]
    fn serve_stdio_returns_error_frame_for_bad_request() {
        let input = b"{\"op\":\"bad\"}\n";
        let mut output = Vec::new();

        serve_network_helper_stdio(&input[..], &mut output).unwrap();

        let lines = String::from_utf8(output).unwrap();
        let first = lines.lines().next().unwrap();
        assert!(first.contains("\"status\":\"err\""));
        assert!(first.contains("\"kind\":\"invalid_request\""));
    }

    #[test]
    fn serve_stdio_rejects_oversized_frame_before_decode() {
        let mut input = vec![b' '; NETWORK_HELPER_MAX_FRAME_BYTES + 1];
        input.push(b'\n');
        let mut output = Vec::new();

        serve_network_helper_stdio(&input[..], &mut output).unwrap();

        let lines = String::from_utf8(output).unwrap();
        let first = lines.lines().next().unwrap();
        assert!(first.contains("\"status\":\"err\""));
        assert!(first.contains("\"kind\":\"invalid_request\""));
        assert!(first.contains("exceeds"));
    }
}
