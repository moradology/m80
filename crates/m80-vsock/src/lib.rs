//! Host-side bridge between Firecracker UDS and the guest's vsock CID/port.
//!
//! See `README.md` for the black-box contract.
//! Behavior captures: bead epic `m80-xjg` (`br show m80-xjg`).

#![deny(missing_docs)]

use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde::de::DeserializeOwned;
use sha2::Digest;

use m80_proto::{Envelope, ProtoError};

/// Re-export the canonical defaults from `m80-proto` so callers that already
/// depend on `m80-vsock` don't need a separate import.
pub use m80_proto::{GUEST_PORT_DEFAULT, READY_MARKER_DEFAULT};

/// Read/write timeout applied to every vsock bridge stream.
const BRIDGE_IO_TIMEOUT: Duration = Duration::from_secs(5);

/// Poll interval used by [`watch_ready_marker`].
const READY_POLL_INTERVAL: Duration = Duration::from_millis(50);

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

/// Watch `console` for `marker` on its own line, polling every 50 ms.
///
/// Returns `Ok(())` as soon as the marker is found, or
/// [`VsockError::NotReady`] if `timeout` elapses first.
///
/// Exposed as `pub` for integration testing; the primary entry point is
/// [`Channel::open`], which calls this automatically.
pub fn watch_ready_marker(
    console: &Path,
    marker: &str,
    timeout: Duration,
) -> Result<(), VsockError> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Ok(contents) = std::fs::read_to_string(console) {
            if contents.lines().any(|line| line == marker) {
                return Ok(());
            }
        }
        if Instant::now() >= deadline {
            return Err(VsockError::NotReady);
        }
        std::thread::sleep(READY_POLL_INTERVAL);
    }
}

/// One open connection to the in-VM daemon. Created by [`Channel::open`];
/// dropped when the request/response cycle is done.
pub struct Channel {
    /// The host-side UDS path, removed on close/drop.
    host_uds: PathBuf,
    /// Raw stream used for writing (kept separate from `buf_reader`).
    stream: UnixStream,
    /// Buffered reader wrapping a clone of `stream` for `read_frame`.
    buf_reader: BufReader<UnixStream>,
    /// Whether `close` has already been called (guard for Drop).
    closed: bool,
}

impl std::fmt::Debug for Channel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Channel")
            .field("host_uds", &self.host_uds)
            .field("closed", &self.closed)
            .finish_non_exhaustive()
    }
}

impl Channel {
    /// Watch `console` for `ready_marker`, then connect to `host_uds` and
    /// hand-shake to the guest's vsock `guest_port`. `timeout` bounds the
    /// ready-watch step.
    ///
    /// The default port is [`GUEST_PORT_DEFAULT`] and the default marker is
    /// [`READY_MARKER_DEFAULT`]; callers typically pass those directly.
    pub fn open(
        host_uds: &Path,
        guest_port: u32,
        ready_marker: &str,
        console: &Path,
        timeout: Duration,
    ) -> Result<Self, VsockError> {
        watch_ready_marker(console, ready_marker, timeout)?;

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
        }

        // Read OK response.
        let reader_stream = stream.try_clone().map_err(VsockError::Io)?;
        let mut buf_reader = BufReader::new(reader_stream);
        let mut ack = String::new();
        buf_reader.read_line(&mut ack).map_err(VsockError::Io)?;
        if !ack.starts_with("OK ") {
            return Err(VsockError::HandshakeFailed);
        }

        Ok(Channel {
            host_uds: host_uds.to_owned(),
            stream,
            buf_reader,
            closed: false,
        })
    }

    /// Send one [`Envelope`] over the channel.
    pub fn send<T: Serialize>(&mut self, envelope: &Envelope<T>) -> Result<(), VsockError> {
        m80_proto::write_frame(&mut self.stream, envelope)?;
        Ok(())
    }

    /// Receive one [`Envelope`] from the channel.
    pub fn recv<U: DeserializeOwned>(&mut self) -> Result<Envelope<U>, VsockError> {
        let envelope = m80_proto::read_frame(&mut self.buf_reader)?;
        Ok(envelope)
    }

    /// Close the connection and remove the host-side UDS. Idempotent.
    pub fn close(mut self) -> Result<(), VsockError> {
        self.closed = true;
        self.stream.flush().map_err(VsockError::Io)?;
        std::fs::remove_file(&self.host_uds).or_else(|e| {
            if e.kind() == io::ErrorKind::NotFound {
                Ok(())
            } else {
                Err(VsockError::Io(e))
            }
        })?;
        Ok(())
    }

    /// Perform cleanup, swallowing errors (used by Drop).
    fn cleanup(&mut self) {
        let _ = self.stream.flush();
        if let Err(e) = std::fs::remove_file(&self.host_uds) {
            if e.kind() != io::ErrorKind::NotFound {
                tracing::warn!(path = %self.host_uds.display(), err = %e, "vsock UDS removal failed during drop");
            }
        }
    }
}

impl Drop for Channel {
    fn drop(&mut self) {
        if !self.closed {
            self.cleanup();
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
