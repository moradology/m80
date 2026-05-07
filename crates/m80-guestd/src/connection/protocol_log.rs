//! Guest-side protocol diagnostic formatting.

use m80_proto::ProtoError;

use crate::guest_log::{self, GuestLogPhase};

const HOST_PEER: &str = "host-vsock";

pub(super) fn warn_proto_error(
    phase: GuestLogPhase,
    request_id: Option<&str>,
    stream_id: Option<&str>,
    error: &ProtoError,
) {
    guest_log::warn(
        phase,
        request_id,
        proto_error_message(request_id, stream_id, error),
    );
}

pub(super) fn warn_unexpected_frame(
    phase: GuestLogPhase,
    request_id: Option<&str>,
    stream_id: Option<&str>,
    kind: &str,
) {
    guest_log::warn(
        phase,
        request_id,
        unexpected_frame_message(request_id, stream_id, kind),
    );
}

pub(super) fn warn_sequence_mismatch(
    phase: GuestLogPhase,
    request_id: Option<&str>,
    stream_id: &str,
    expected: u64,
    got: u64,
) {
    guest_log::warn(
        phase,
        request_id,
        sequence_mismatch_message(request_id, stream_id, expected, got),
    );
}

fn proto_error_message(
    request_id: Option<&str>,
    stream_id: Option<&str>,
    error: &ProtoError,
) -> String {
    format!(
        "protocol_error peer={} request_id={} stream_id={} frame_size={} error_class={} detail={}",
        HOST_PEER,
        request_id.unwrap_or("unknown"),
        stream_id.unwrap_or("none"),
        frame_size(error),
        error_class(error),
        error
    )
}

fn unexpected_frame_message(
    request_id: Option<&str>,
    stream_id: Option<&str>,
    kind: &str,
) -> String {
    format!(
        "protocol_error peer={} request_id={} stream_id={} frame_size=unknown error_class=unexpected_frame kind={}",
        HOST_PEER,
        request_id.unwrap_or("unknown"),
        stream_id.unwrap_or("none"),
        kind
    )
}

fn sequence_mismatch_message(
    request_id: Option<&str>,
    stream_id: &str,
    expected: u64,
    got: u64,
) -> String {
    format!(
        "protocol_error peer={} request_id={} stream_id={} frame_size=unknown error_class=sequence_mismatch expected={} got={}",
        HOST_PEER,
        request_id.unwrap_or("unknown"),
        stream_id,
        expected,
        got
    )
}

fn frame_size(error: &ProtoError) -> String {
    match error {
        ProtoError::OversizedPayload { size, .. } => size.to_string(),
        _ => "unknown".to_string(),
    }
}

fn error_class(error: &ProtoError) -> &'static str {
    match error {
        ProtoError::IncompatibleVersion { .. } => "version_mismatch",
        ProtoError::MalformedPayload(_) => "malformed_frame",
        ProtoError::EncodeFailed(_) => "encode_failed",
        ProtoError::OversizedPayload { .. } => "oversized_frame",
        ProtoError::Io(_) => "io",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_frame_log_carries_peer_request_stream_and_size_fields() {
        let msg = proto_error_message(
            Some("req-7"),
            Some("exec_request"),
            &ProtoError::MalformedPayload("bad wire type".into()),
        );

        assert!(msg.contains("protocol_error peer=host-vsock"));
        assert!(msg.contains("request_id=req-7"));
        assert!(msg.contains("stream_id=exec_request"));
        assert!(msg.contains("frame_size=unknown"));
        assert!(msg.contains("error_class=malformed_frame"));
        assert!(msg.contains("detail=malformed payload: bad wire type"));
    }

    #[test]
    fn oversized_frame_log_carries_observed_size() {
        let msg = proto_error_message(
            None,
            None,
            &ProtoError::OversizedPayload {
                size: 5_242_881,
                limit: 4_194_304,
            },
        );

        assert!(msg.contains("request_id=unknown"));
        assert!(msg.contains("stream_id=none"));
        assert!(msg.contains("frame_size=5242881"));
        assert!(msg.contains("error_class=oversized_frame"));
    }

    #[test]
    fn version_mismatch_log_reports_expected_and_got() {
        let msg = proto_error_message(
            Some("req-8"),
            Some("exec_stdout"),
            &ProtoError::IncompatibleVersion {
                expected: 1,
                got: 2,
            },
        );

        assert!(msg.contains("error_class=version_mismatch"));
        assert!(msg.contains("detail=protocol version mismatch: expected 1, got 2"));
    }

    #[test]
    fn unexpected_frame_log_names_kind() {
        let msg = unexpected_frame_message(Some("req-9"), Some("control"), "file_read_chunk");

        assert!(msg.contains("peer=host-vsock"));
        assert!(msg.contains("request_id=req-9"));
        assert!(msg.contains("stream_id=control"));
        assert!(msg.contains("error_class=unexpected_frame"));
        assert!(msg.contains("kind=file_read_chunk"));
    }

    #[test]
    fn sequence_mismatch_log_reports_expected_and_got() {
        let msg = sequence_mismatch_message(Some("req-11"), "pty_input", 2, 7);

        assert!(msg.contains("peer=host-vsock"));
        assert!(msg.contains("request_id=req-11"));
        assert!(msg.contains("stream_id=pty_input"));
        assert!(msg.contains("error_class=sequence_mismatch"));
        assert!(msg.contains("expected=2"));
        assert!(msg.contains("got=7"));
    }

    #[test]
    fn guest_stderr_line_preserves_protocol_fields() {
        let msg = proto_error_message(
            Some("req-10"),
            Some("exec_exit"),
            &ProtoError::MalformedPayload("missing terminal".into()),
        );
        let line = crate::guest_log::format_line(
            "2026-05-07T00:00:00Z",
            GuestLogPhase::Exec,
            Some("req-10"),
            crate::guest_log::GuestLogLevel::Warn,
            &msg,
        );

        assert!(line.starts_with("[2026-05-07T00:00:00Z] [Exec] [req-10] WARN "));
        assert!(line.contains("protocol_error peer=host-vsock"));
        assert!(line.contains("request_id=req-10"));
        assert!(line.contains("stream_id=exec_exit"));
        assert!(line.contains("error_class=malformed_frame"));
    }
}
