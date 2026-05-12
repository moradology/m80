//! Guest health methods for [`RunningSandbox`].

use std::sync::atomic::Ordering;

use m80_proto::{Envelope, PingRequest, PongResponse};

use crate::error::{FcError, WireProtocolError};
use crate::layout::VSOCK_SOCKET;
use crate::lifecycle::exec::{request_id_for, send_envelope_with_open_retry};
use crate::lifecycle::monotonic_ns;
use crate::types::RunningSandbox;

impl RunningSandbox {
    /// Send one direct health probe to m80-guestd.
    pub fn ping_guest(&mut self) -> Result<PongResponse, FcError> {
        if self.idle_timed_out.load(Ordering::Relaxed) {
            return Err(FcError::IdleTimedOut);
        }
        self.last_activity_ns
            .store(monotonic_ns(), Ordering::Relaxed);
        let vsock_uds = self.jail.jail_root().join(VSOCK_SOCKET);
        let request_id = request_id_for(&self.vm_id, self.request_id.as_deref(), "ping");
        let envelope = Envelope::with_request_id(PingRequest {}, request_id.clone());
        let mut channel = send_envelope_with_open_retry(&vsock_uds, &self.vm_id, &envelope)?;
        let frame: Envelope<PongResponse> = channel.recv()?;
        let response = validate_pong_response(frame, &request_id)?;
        self.last_activity_ns
            .store(monotonic_ns(), Ordering::Relaxed);
        Ok(response)
    }
}

fn validate_pong_response(
    frame: Envelope<PongResponse>,
    request_id: &str,
) -> Result<PongResponse, FcError> {
    if frame.request_id.as_deref() != Some(request_id) {
        return Err(FcError::Protocol(WireProtocolError::MalformedPeer(
            format!(
                "pong request_id mismatch: expected {request_id:?}, got {:?}",
                frame.request_id
            ),
        )));
    }
    Ok(frame.payload)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_pong_response_requires_matching_request_id() {
        let frame = Envelope::with_request_id(
            PongResponse { guest_unix_ms: 42 },
            "wrong-request".to_owned(),
        );

        let err = validate_pong_response(frame, "req-ping").unwrap_err();

        assert!(matches!(
            err,
            FcError::Protocol(WireProtocolError::MalformedPeer(detail))
                if detail.contains("pong request_id mismatch")
        ));
    }

    #[test]
    fn validate_pong_response_returns_payload() {
        let frame =
            Envelope::with_request_id(PongResponse { guest_unix_ms: 42 }, "req-ping".to_owned());

        let pong = validate_pong_response(frame, "req-ping").unwrap();

        assert_eq!(pong.guest_unix_ms, 42);
    }
}
