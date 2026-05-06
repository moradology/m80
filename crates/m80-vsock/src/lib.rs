//! Host-side bridge between Firecracker UDS and the guest's vsock CID/port.
//!
//! See `README.md` for the black-box contract.
//! Behavior captures: bead epic `m80-xjg` (`br show m80-xjg`).

#![deny(missing_docs)]

mod debug_wire;

use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde::Serialize;
use sha2::Digest;

use m80_proto::{Envelope, ProtoError};

/// Re-export the canonical default vsock port from `m80-proto`.
pub use m80_proto::GUEST_PORT_DEFAULT;

/// Read/write timeout applied to every vsock bridge stream.
const BRIDGE_IO_TIMEOUT: Duration = Duration::from_secs(5);

/// Derive the vsock CID a VM should be assigned from its `vm_id`.
///
/// Pure: no I/O, no allocation table. Two VMs with the same `vm_id` get the
/// same CID; different `vm_id`s get distinct CIDs (modulo the 32-bit space).
/// CID is always in the range `3..=u32::MAX`; Firecracker reserves 0, 1, and 2.
pub fn cid_for_vm_id(vm_id: &str) -> u32 {
    let mut hasher = sha2::Sha256::new();
    hasher.update(vm_id.as_bytes());
    let bytes = hasher.finalize();
    let raw = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    3 + (raw % (u32::MAX - 2))
}

/// One open connection to the in-VM daemon. Created by [`Channel::open_uds_only`];
/// dropped when the request/response cycle is done.
pub struct Channel {
    /// The host-side Firecracker UDS path. This is a listener owned by the VM,
    /// so channel teardown must not unlink it.
    host_uds: PathBuf,
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
    host_uds: PathBuf,
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

impl Channel {
    /// Connect to `host_uds` and hand-shake to `guest_port`. Readiness is
    /// established by the caller via [`m80_proto::READY_PORT_DEFAULT`]'s
    /// inverted-readiness vsock signal before this is called; this just
    /// opens the exec channel.
    pub fn open_uds_only(host_uds: &Path, guest_port: u32) -> Result<Self, VsockError> {
        let stream = UnixStream::connect(host_uds).map_err(|e| VsockError::ConnectFailed {
            errno: e.raw_os_error().unwrap_or(0),
        })?;

        stream
            .set_read_timeout(Some(BRIDGE_IO_TIMEOUT))
            .map_err(VsockError::Io)?;
        stream
            .set_write_timeout(Some(BRIDGE_IO_TIMEOUT))
            .map_err(VsockError::Io)?;

        // Write CONNECT line.
        {
            let mut w = &stream;
            writeln!(w, "CONNECT {guest_port}").map_err(VsockError::Io)?;
            w.flush().map_err(VsockError::Io)?;
            if debug_wire::is_enabled("vsock") {
                let line = format!("CONNECT {guest_port}\n");
                tracing::trace!(direction = "out", msg = line.trim(), "vsock handshake");
            }
        }

        // Read OK response.
        let reader_stream = stream.try_clone().map_err(VsockError::Io)?;
        let mut buf_reader = BufReader::new(reader_stream);
        let mut ack = String::new();
        buf_reader.read_line(&mut ack).map_err(VsockError::Io)?;
        if debug_wire::is_enabled("vsock") {
            tracing::trace!(direction = "in", msg = ack.trim(), "vsock handshake");
        }
        if !ack.starts_with("OK ") {
            return Err(VsockError::HandshakeFailed);
        }

        Ok(Channel {
            host_uds: host_uds.to_owned(),
            stream,
            buf_reader,
        })
    }

    /// Send one [`Envelope`] over the channel.
    pub fn send<T: Serialize>(&mut self, envelope: &Envelope<T>) -> Result<(), VsockError> {
        if debug_wire::is_enabled("vsock") {
            // Serialize only when the gate fires to avoid allocation on the default path.
            if let Ok(bytes) = serde_json::to_vec(envelope) {
                tracing::trace!(
                    direction = "out",
                    preview = %debug_wire::format_wire_preview(&bytes),
                    "vsock frame"
                );
            }
        }
        m80_proto::write_frame(&mut self.stream, envelope)?;
        Ok(())
    }

    /// Clone a write-only sender for the same underlying connection.
    ///
    /// Guestd cancellation is same-connection: a second UDS connection would
    /// sit behind the in-flight exec and could not interrupt it.
    pub fn try_clone_sender(&self) -> Result<ChannelSender, VsockError> {
        Ok(ChannelSender {
            host_uds: self.host_uds.clone(),
            stream: self.stream.try_clone().map_err(VsockError::Io)?,
        })
    }

    /// Receive one [`Envelope`] from the channel.
    ///
    /// When `M80_DEBUG_WIRE=vsock` (or `all`) is set, this takes a separate
    /// read path that captures the raw NDJSON line into memory before parsing,
    /// so the bytes can be logged. The default path uses `read_frame` directly
    /// against the buffered reader. Keep both paths in sync if `read_frame`'s
    /// framing assumptions change.
    pub fn recv<U: DeserializeOwned>(&mut self) -> Result<Envelope<U>, VsockError> {
        if debug_wire::is_enabled("vsock") {
            // Capture the raw NDJSON line for logging, then deserialize from the
            // captured bytes. The limit mirrors the cap inside `read_frame`.
            let limit = m80_proto::MAX_FRAME_BYTES as u64 + 2;
            let mut raw_line: Vec<u8> = Vec::with_capacity(256);
            self.buf_reader
                .by_ref()
                .take(limit)
                .read_until(b'\n', &mut raw_line)
                .map_err(VsockError::Io)?;
            tracing::trace!(
                direction = "in",
                preview = %debug_wire::format_wire_preview(&raw_line),
                "vsock frame"
            );
            let envelope = m80_proto::read_frame(&mut std::io::Cursor::new(raw_line))?;
            return Ok(envelope);
        }
        let envelope = m80_proto::read_frame(&mut self.buf_reader)?;
        Ok(envelope)
    }

    /// Flush the connection stream. The host-side UDS is owned by Firecracker
    /// and remains in place for subsequent connections.
    fn teardown(&mut self) -> Result<(), VsockError> {
        self.stream.flush().map_err(VsockError::Io)
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
            tracing::warn!(path = %self.host_uds.display(), err = %e, "vsock connection teardown failed during drop");
        }
    }
}

impl ChannelSender {
    /// Send one [`Envelope`] over the cloned write half.
    pub fn send<T: Serialize>(&mut self, envelope: &Envelope<T>) -> Result<(), VsockError> {
        if debug_wire::is_enabled("vsock") {
            if let Ok(bytes) = serde_json::to_vec(envelope) {
                tracing::trace!(
                    direction = "out",
                    preview = %debug_wire::format_wire_preview(&bytes),
                    "vsock frame"
                );
            }
        }
        m80_proto::write_frame(&mut self.stream, envelope)?;
        Ok(())
    }

    /// Flush the cloned sender.
    pub fn close(mut self) -> Result<(), VsockError> {
        self.stream.flush().map_err(VsockError::Io)
    }
}

impl Drop for ChannelSender {
    fn drop(&mut self) {
        if let Err(e) = self.stream.flush() {
            tracing::warn!(path = %self.host_uds.display(), err = %e, "vsock sender teardown failed during drop");
        }
    }
}

/// Errors surfaced by [`Channel`] operations.
#[derive(Debug, thiserror::Error)]
pub enum VsockError {
    /// Ready marker was not seen on the serial console within the timeout.
    #[error("guestd ready marker not observed before timeout")]
    NotReady,
    /// Connect to the Firecracker UDS failed.
    #[error("vsock connect failed (errno={errno})")]
    ConnectFailed {
        /// libc errno reported by the connect call.
        errno: i32,
    },
    /// The Firecracker UDS-to-vsock handshake was malformed.
    #[error("vsock handshake failed")]
    HandshakeFailed,
    /// Underlying I/O failure.
    #[error("i/o: {0}")]
    Io(#[from] io::Error),
    /// Frame-level protocol error.
    #[error("proto: {0}")]
    Proto(#[from] ProtoError),
}
