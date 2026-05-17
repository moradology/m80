use std::io;
use std::time::{Duration, Instant};

use m80_proto::ProtoError;
use m80_vsock::VsockError;

use crate::error::{DisconnectCause, FcError, WireProtocolError};

const DISCONNECT_DEAD_PID_GRACE: Duration = Duration::from_millis(100);
const DISCONNECT_DEAD_PID_POLL: Duration = Duration::from_millis(1);

pub(super) fn recv_error(err: VsockError, context: &'static str, firecracker_pid: u32) -> FcError {
    match err {
        VsockError::Proto(ProtoError::Io(err)) if err.kind() == io::ErrorKind::UnexpectedEof => {
            disconnect_before_terminal(
                context,
                observed_disconnect_cause(firecracker_pid, DisconnectCause::MidStreamEof),
            )
        }
        VsockError::Proto(ProtoError::Io(err)) if is_read_timeout(err.kind()) => {
            FcError::Protocol(WireProtocolError::ReadTimeout { context })
        }
        VsockError::Proto(err) => proto_error(err),
        VsockError::Io { source, .. } if source.kind() == io::ErrorKind::UnexpectedEof => {
            disconnect_before_terminal(
                context,
                observed_disconnect_cause(firecracker_pid, DisconnectCause::MidStreamEof),
            )
        }
        VsockError::Io { source, .. } if is_read_timeout(source.kind()) => {
            FcError::Protocol(WireProtocolError::ReadTimeout { context })
        }
        other => FcError::Vsock(other),
    }
}

pub(super) fn recv_error_after_clean_request(
    err: VsockError,
    context: &'static str,
    firecracker_pid: u32,
) -> FcError {
    match err {
        VsockError::Proto(ProtoError::Io(err)) if err.kind() == io::ErrorKind::UnexpectedEof => {
            disconnect_before_terminal(
                context,
                observed_disconnect_cause(firecracker_pid, DisconnectCause::CleanRequestedClose),
            )
        }
        VsockError::Io { source, .. } if source.kind() == io::ErrorKind::UnexpectedEof => {
            disconnect_before_terminal(
                context,
                observed_disconnect_cause(firecracker_pid, DisconnectCause::CleanRequestedClose),
            )
        }
        other => recv_error(other, context, firecracker_pid),
    }
}

pub(super) fn open_send_error(
    err: VsockError,
    context: &'static str,
    firecracker_pid: u32,
) -> FcError {
    if is_transient_open_send_disconnect(&err) {
        return disconnect_before_terminal(
            context,
            observed_disconnect_cause(firecracker_pid, DisconnectCause::UdsConnectFailed),
        );
    }
    FcError::Vsock(err)
}

pub(super) fn sandbox_dead(vm_id: &str, firecracker_pid: u32) -> FcError {
    FcError::SandboxDead {
        vm_id: vm_id.to_owned(),
        firecracker_pid,
    }
}

pub(super) fn ensure_firecracker_live(vm_id: &str, firecracker_pid: u32) -> Result<(), FcError> {
    if crate::runroot::pid_is_alive(firecracker_pid) {
        Ok(())
    } else {
        Err(sandbox_dead(vm_id, firecracker_pid))
    }
}

fn observed_disconnect_cause(firecracker_pid: u32, live_cause: DisconnectCause) -> DisconnectCause {
    if !crate::runroot::pid_is_alive(firecracker_pid) {
        return DisconnectCause::FcProcessDead;
    }

    let deadline = Instant::now() + DISCONNECT_DEAD_PID_GRACE;
    while Instant::now() < deadline {
        std::thread::sleep(DISCONNECT_DEAD_PID_POLL);
        if !crate::runroot::pid_is_alive(firecracker_pid) {
            return DisconnectCause::FcProcessDead;
        }
    }

    live_cause
}

fn disconnect_before_terminal(context: &'static str, cause: DisconnectCause) -> FcError {
    FcError::Protocol(WireProtocolError::DisconnectBeforeTerminal { context, cause })
}

fn is_transient_open_send_disconnect(err: &VsockError) -> bool {
    match err {
        VsockError::HandshakeFailed => true,
        VsockError::Io { source, .. } => matches!(
            source.kind(),
            io::ErrorKind::BrokenPipe | io::ErrorKind::ConnectionRefused
        ),
        _ => false,
    }
}

fn is_read_timeout(kind: io::ErrorKind) -> bool {
    matches!(kind, io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock)
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
            std::process::id(),
        );

        assert!(matches!(
            err,
            FcError::Protocol(WireProtocolError::DisconnectBeforeTerminal {
                context: "streaming exec",
                cause: DisconnectCause::MidStreamEof
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
            std::process::id(),
        );

        assert!(matches!(
            err,
            FcError::Protocol(WireProtocolError::DisconnectBeforeTerminal {
                context: "streaming exec",
                cause: DisconnectCause::MidStreamEof
            })
        ));
    }

    #[test]
    fn unexpected_eof_reports_dead_firecracker_cause_when_pid_is_gone() {
        let err = recv_error(
            VsockError::Io {
                path: std::path::PathBuf::new(),
                source: io::Error::from(io::ErrorKind::UnexpectedEof),
            },
            "streaming exec",
            u32::MAX,
        );

        assert!(matches!(
            err,
            FcError::Protocol(WireProtocolError::DisconnectBeforeTerminal {
                context: "streaming exec",
                cause: DisconnectCause::FcProcessDead
            })
        ));
    }

    #[test]
    fn connection_refused_open_send_reports_uds_connect_cause_when_pid_is_live() {
        let err = open_send_error(
            VsockError::Io {
                path: std::path::PathBuf::new(),
                source: io::Error::from(io::ErrorKind::ConnectionRefused),
            },
            "exec send",
            std::process::id(),
        );

        assert!(matches!(
            err,
            FcError::Protocol(WireProtocolError::DisconnectBeforeTerminal {
                context: "exec send",
                cause: DisconnectCause::UdsConnectFailed
            })
        ));
    }

    #[test]
    fn ensure_firecracker_live_returns_sandbox_dead_for_missing_pid() {
        let err = ensure_firecracker_live("vm-dead", u32::MAX).unwrap_err();

        assert!(matches!(
            err,
            FcError::SandboxDead {
                vm_id,
                firecracker_pid: u32::MAX
            } if vm_id == "vm-dead"
        ));
    }

    #[test]
    fn eof_after_clean_request_reports_clean_close_when_pid_is_live() {
        let err = recv_error_after_clean_request(
            VsockError::Io {
                path: std::path::PathBuf::new(),
                source: io::Error::from(io::ErrorKind::UnexpectedEof),
            },
            "shutdown_response",
            std::process::id(),
        );

        assert!(matches!(
            err,
            FcError::Protocol(WireProtocolError::DisconnectBeforeTerminal {
                context: "shutdown_response",
                cause: DisconnectCause::CleanRequestedClose
            })
        ));
    }

    #[test]
    fn read_timeout_maps_to_protocol_timeout() {
        let err = recv_error(
            VsockError::Io {
                path: std::path::PathBuf::new(),
                source: io::Error::from(io::ErrorKind::TimedOut),
            },
            "streaming exec",
            std::process::id(),
        );

        assert!(matches!(
            err,
            FcError::Protocol(WireProtocolError::ReadTimeout {
                context: "streaming exec"
            })
        ));
    }

    #[test]
    fn proto_would_block_maps_to_protocol_timeout() {
        let err = recv_error(
            VsockError::Proto(ProtoError::Io(io::Error::from(io::ErrorKind::WouldBlock))),
            "streaming exec",
            std::process::id(),
        );

        assert!(matches!(
            err,
            FcError::Protocol(WireProtocolError::ReadTimeout {
                context: "streaming exec"
            })
        ));
    }
}
