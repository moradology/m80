//! Guest metrics methods for [`RunningSandbox`].

use std::sync::atomic::Ordering;

use m80_proto::{Envelope, MetricsRequest, MetricsResponse};

use crate::error::FcError;
use crate::layout::VSOCK_SOCKET;
use crate::lifecycle::exec::{request_id_for, send_envelope_with_open_retry};
use crate::lifecycle::monotonic_ns;
use crate::types::RunningSandbox;

impl RunningSandbox {
    /// Read guest-side metrics directly from m80-guestd.
    pub fn guest_metrics(&mut self) -> Result<MetricsResponse, FcError> {
        if self.idle_timed_out.load(Ordering::Relaxed) {
            return Err(FcError::IdleTimedOut);
        }
        self.last_activity_ns
            .store(monotonic_ns(), Ordering::Relaxed);
        let vsock_uds = self.jail.jail_root().join(VSOCK_SOCKET);
        let request_id = request_id_for(&self.vm_id, self.request_id.as_deref(), "metrics");
        let envelope = Envelope::with_request_id(MetricsRequest {}, request_id);
        let firecracker_pid = self.firecracker.firecracker_pid();
        let mut channel = send_envelope_with_open_retry(
            &vsock_uds,
            &self.vm_id,
            firecracker_pid,
            "guest metrics",
            &envelope,
        )?;
        let frame: Envelope<MetricsResponse> = channel
            .recv()
            .map_err(|e| super::protocol::recv_error(e, "guest metrics", firecracker_pid))?;
        let response = frame.payload;
        self.last_activity_ns
            .store(monotonic_ns(), Ordering::Relaxed);
        Ok(response)
    }
}
