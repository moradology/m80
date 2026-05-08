use std::io;

use m80_proto::ProtoError;
use m80_vsock::VsockError;

use crate::error::{FcError, WireProtocolError};

pub(super) fn recv_error(err: VsockError, context: &'static str) -> FcError {
    match err {
        VsockError::Proto(ProtoError::Io(err)) if err.kind() == io::ErrorKind::UnexpectedEof => {
            FcError::Protocol(WireProtocolError::DisconnectBeforeTerminal { context })
        }
        VsockError::Proto(err) => proto_error(err),
        VsockError::Io { source, .. } if source.kind() == io::ErrorKind::UnexpectedEof => {
            FcError::Protocol(WireProtocolError::DisconnectBeforeTerminal { context })
        }
        other => FcError::Vsock(other),
    }
}

pub(super) fn proto_error(err: ProtoError) -> FcError {
    match err {
        ProtoError::MalformedPayload(detail) => {
            FcError::Protocol(WireProtocolError::MalformedPeer(detail))
        }
        ProtoError::OversizedPayload { size, limit } => {
            FcError::Protocol(WireProtocolError::OversizedFrame { size, limit })
        }
        ProtoError::IncompatibleVersion { expected, got } => {
            FcError::Protocol(WireProtocolError::UnsupportedVersion { expected, got })
        }
        other => FcError::Vsock(VsockError::Proto(other)),
    }
}

pub(super) fn unexpected_frame(
    context: &'static str,
    expected: &'static str,
    got: impl Into<String>,
) -> FcError {
    FcError::Protocol(WireProtocolError::UnexpectedFrame {
        context,
        expected,
        got: got.into(),
    })
}

pub(super) fn request_id_mismatch(
    context: &'static str,
    expected: &str,
    got: Option<String>,
) -> FcError {
    FcError::Protocol(WireProtocolError::RequestIdMismatch {
        context,
        expected: expected.to_owned(),
        got,
    })
}

pub(super) fn sequence_mismatch(stream: &'static str, expected: u64, got: u64) -> FcError {
    FcError::Protocol(WireProtocolError::SequenceMismatch {
        stream,
        expected,
        got,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use m80_proto::MAX_FRAME_BYTES;

    #[test]
    fn oversized_proto_maps_to_protocol_error() {
        let err = proto_error(ProtoError::OversizedPayload {
            size: MAX_FRAME_BYTES + 1,
            limit: MAX_FRAME_BYTES,
        });

        assert!(matches!(
            err,
            FcError::Protocol(WireProtocolError::OversizedFrame { size, limit })
                if size == MAX_FRAME_BYTES + 1 && limit == MAX_FRAME_BYTES
        ));
    }

    #[test]
    fn unexpected_eof_maps_to_disconnect_before_terminal() {
        let err = recv_error(
            VsockError::Io {
                path: std::path::PathBuf::new(),
                source: io::Error::from(io::ErrorKind::UnexpectedEof),
            },
            "streaming exec",
        );

        assert!(matches!(
            err,
            FcError::Protocol(WireProtocolError::DisconnectBeforeTerminal {
                context: "streaming exec"
            })
        ));
    }

    #[test]
    fn proto_unexpected_eof_maps_to_disconnect_before_terminal() {
        let err = recv_error(
            VsockError::Proto(ProtoError::Io(io::Error::from(
                io::ErrorKind::UnexpectedEof,
            ))),
            "streaming exec",
        );

        assert!(matches!(
            err,
            FcError::Protocol(WireProtocolError::DisconnectBeforeTerminal {
                context: "streaming exec"
            })
        ));
    }
}
