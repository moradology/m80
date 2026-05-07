//! Host-side bridge between Firecracker UDS and the guest's vsock CID/port.
//!
//! See `README.md` for the black-box contract.
//! Behavior captures: bead epic `m80-xjg` (`br show m80-xjg`).

#![deny(missing_docs)]

mod debug_wire;

use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use sha2::Digest;

use m80_proto::{encode_raw_envelope, Envelope, Payload, ProtoError, RawEnvelope};

/// Re-export the canonical default vsock port from `m80-proto`.
pub use m80_proto::GUEST_PORT_DEFAULT;

/// Read/write timeout applied to every vsock bridge stream.
const BRIDGE_IO_TIMEOUT: Duration = Duration::from_secs(5);

/// Derive the vsock CID a VM should be assigned from its `vm_id`.
///
/// Pure: no I/O, no allocation table. Two VMs with the same `vm_id` get the
/// same CID; different `vm_id`s get distinct CIDs (modulo the 32-bit space).
/// CID is always in the range `3..=u32::MAX - 1`; Firecracker reserves
/// 0, 1, 2, and `u32::MAX`.
pub fn cid_for_vm_id(vm_id: &str) -> u32 {
    let mut hasher = sha2::Sha256::new();
    hasher.update(vm_id.as_bytes());
    let bytes = hasher.finalize();
    let raw = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    3 + (raw % (u32::MAX - 3))
}

/// One open connection to the in-VM daemon. Created by [`Channel::open_uds_only`];
/// dropped when the request/response cycle is done.
pub struct Channel {
    /// The host-side Firecracker UDS path. This is a listener owned by the VM,
    /// so channel teardown must not unlink it.
    host_uds: Arc<Path>,
    /// Raw stream used for writing (kept separate from `buf_reader`).
    stream: UnixStream,
    /// Buffered reader wrapping a clone of `stream` for `read_frame`.
    buf_reader: BufReader<UnixStream>,
}

/// Write-only clone of an open [`Channel`].
///
/// This is used for same-connection control frames while the owning
/// [`Channel`] is blocked waiting for response frames.
pub struct ChannelSender {
    /// The host-side Firecracker UDS path. Used only for diagnostics.
    host_uds: Arc<Path>,
    /// Raw cloned stream for writing frames.
    stream: UnixStream,
}

impl std::fmt::Debug for Channel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Channel")
            .field("host_uds", &self.host_uds)
            .finish_non_exhaustive()
    }
}

impl std::fmt::Debug for ChannelSender {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChannelSender")
            .field("host_uds", &self.host_uds)
            .finish_non_exhaustive()
    }
}

/// Write `envelope` to `stream`, emitting a debug-wire trace if enabled.
///
/// Used by both [`Channel::send`] and [`ChannelSender::send`].
fn send_envelope<W, T>(stream: &mut W, envelope: &Envelope<T>) -> Result<(), VsockError>
where
    W: Write,
    T: Payload + Clone,
{
    let raw = RawEnvelope::from_typed(envelope.clone());
    if debug_wire::is_enabled("vsock") {
        let bytes = encode_raw_envelope(raw.clone())?;
        tracing::trace!(
            direction = "out",
            preview = %debug_wire::format_wire_preview(&bytes),
            "vsock frame"
        );
    }
    m80_proto::write_raw_frame(stream, raw)?;
    Ok(())
}

/// Wrap `source` in [`VsockError::Io`] carrying the UDS path.
///
/// Used by every call site that converts an [`io::Error`] coming from a
/// stream operation on `host_uds`.
fn io_err(host_uds: &Arc<Path>, source: io::Error) -> VsockError {
    VsockError::Io {
        path: host_uds.to_path_buf(),
        source,
    }
}

/// Emit a drop-teardown warning. Called from both [`Channel`] and
/// [`ChannelSender`] Drop impls; must not panic.
fn warn_teardown(host_uds: &Path, context: &str, err: impl std::fmt::Display) {
    tracing::warn!(
        path = %host_uds.display(),
        err = %err,
        "{context} teardown failed during drop",
    );
}

impl Channel {
    /// Connect to `host_uds` and hand-shake to `guest_port`. Readiness is
    /// established by the caller via [`m80_proto::READY_PORT_DEFAULT`]'s
    /// inverted-readiness vsock signal before this is called; this just
    /// opens the exec channel.
    pub fn open_uds_only(host_uds: &Path, guest_port: u32) -> Result<Self, VsockError> {
        let stream = UnixStream::connect(host_uds).map_err(|e| VsockError::Io {
            path: host_uds.to_path_buf(),
            source: e,
        })?;

        // Build the Arc<Path> early so we can use io_err throughout this fn.
        let host_uds_arc: Arc<Path> = Arc::from(host_uds);

        stream
            .set_read_timeout(Some(BRIDGE_IO_TIMEOUT))
            .map_err(|e| io_err(&host_uds_arc, e))?;
        stream
            .set_write_timeout(Some(BRIDGE_IO_TIMEOUT))
            .map_err(|e| io_err(&host_uds_arc, e))?;

        // Write CONNECT line.
        {
            let mut w = &stream;
            let line = format!("CONNECT {guest_port}\n");
            w.write_all(line.as_bytes())
                .map_err(|e| io_err(&host_uds_arc, e))?;
            w.flush().map_err(|e| io_err(&host_uds_arc, e))?;
            if debug_wire::is_enabled("vsock") {
                tracing::trace!(direction = "out", msg = line.trim(), "vsock handshake");
            }
        }

        // Read OK response.
        let reader_stream = stream.try_clone().map_err(|e| io_err(&host_uds_arc, e))?;
        let mut buf_reader = BufReader::new(reader_stream);
        let mut ack = String::new();
        buf_reader
            .read_line(&mut ack)
            .map_err(|e| io_err(&host_uds_arc, e))?;
        if debug_wire::is_enabled("vsock") {
            tracing::trace!(direction = "in", msg = ack.trim(), "vsock handshake");
        }
        if !ack.starts_with("OK ") {
            return Err(VsockError::HandshakeFailed);
        }

        Ok(Channel {
            host_uds: host_uds_arc,
            stream,
            buf_reader,
        })
    }

    /// Send one [`Envelope`] over the channel.
    pub fn send<T>(&mut self, envelope: &Envelope<T>) -> Result<(), VsockError>
    where
        T: Payload + Clone,
    {
        send_envelope(&mut self.stream, envelope)
    }

    /// Clone a write-only sender for the same underlying connection.
    ///
    /// Guestd cancellation is same-connection: a second UDS connection would
    /// sit behind the in-flight exec and could not interrupt it.
    pub fn try_clone_sender(&self) -> Result<ChannelSender, VsockError> {
        Ok(ChannelSender {
            host_uds: Arc::clone(&self.host_uds),
            stream: self
                .stream
                .try_clone()
                .map_err(|e| io_err(&self.host_uds, e))?,
        })
    }

    /// Receive one protobuf-framed [`Envelope`] from the channel.
    pub fn recv<U>(&mut self) -> Result<Envelope<U>, VsockError>
    where
        U: Payload,
    {
        let envelope: Envelope<U> = m80_proto::read_frame(&mut self.buf_reader)?;
        if debug_wire::is_enabled("vsock") {
            tracing::trace!(direction = "in", kind = %envelope.kind, "vsock frame");
        }
        Ok(envelope)
    }

    /// Receive one protobuf frame without choosing the payload type first.
    pub fn recv_raw(&mut self) -> Result<RawEnvelope, VsockError> {
        let envelope = m80_proto::read_raw_frame(&mut self.buf_reader)?;
        if debug_wire::is_enabled("vsock") {
            tracing::trace!(direction = "in", kind = %envelope.kind, "vsock frame");
        }
        Ok(envelope)
    }

    /// Flush the connection stream. The host-side UDS is owned by Firecracker
    /// and remains in place for subsequent connections.
    fn teardown(&mut self) -> Result<(), VsockError> {
        self.stream.flush().map_err(|e| io_err(&self.host_uds, e))
    }

    /// Close the connection. The VM's host-side UDS remains available for the
    /// next channel.
    pub fn close(mut self) -> Result<(), VsockError> {
        self.teardown()
    }
}

impl Drop for Channel {
    fn drop(&mut self) {
        if let Err(e) = self.teardown() {
            warn_teardown(&self.host_uds, "vsock connection", e);
        }
    }
}

impl ChannelSender {
    /// Send one [`Envelope`] over the cloned write half.
    pub fn send<T>(&mut self, envelope: &Envelope<T>) -> Result<(), VsockError>
    where
        T: Payload + Clone,
    {
        send_envelope(&mut self.stream, envelope)
    }

    /// Flush the cloned sender.
    pub fn close(mut self) -> Result<(), VsockError> {
        self.stream.flush().map_err(|e| io_err(&self.host_uds, e))
    }
}

impl Drop for ChannelSender {
    fn drop(&mut self) {
        if let Err(e) = self.stream.flush() {
            warn_teardown(&self.host_uds, "vsock sender", e);
        }
    }
}

/// Errors surfaced by [`Channel`] operations.
#[derive(Debug, thiserror::Error)]
pub enum VsockError {
    /// Guest daemon readiness signal did not arrive before the timeout.
    #[error("guestd readiness signal not observed before timeout")]
    NotReady,
    /// The Firecracker UDS-to-vsock handshake was malformed.
    #[error("vsock handshake failed")]
    HandshakeFailed,
    /// Underlying I/O failure; carries the UDS path so callers don't have to
    /// guess which socket operation failed.
    #[error("i/o on {}: {source}", path.display())]
    Io {
        /// UDS path the I/O was attempted against.
        path: PathBuf,
        /// Underlying I/O error.
        #[source]
        source: io::Error,
    },
    /// Frame-level protocol error.
    #[error("proto: {0}")]
    Proto(#[from] ProtoError),
}
