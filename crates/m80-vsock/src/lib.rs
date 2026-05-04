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

/// Watch `console` for `marker` on its own line. Reads incrementally so a
/// multi-MB serial-console log isn't re-read on each poll; tolerates the
/// file not yet existing (Firecracker may not have created it).
pub fn watch_ready_marker(
    console: &Path,
    marker: &str,
    timeout: Duration,
) -> Result<(), VsockError> {
    use std::io::{BufRead, BufReader, Seek, SeekFrom};

    let deadline = Instant::now() + timeout;
    let mut reader: Option<BufReader<std::fs::File>> = None;
    let mut last_offset: u64 = 0;

    loop {
        if reader.is_none() {
            if let Ok(f) = std::fs::File::open(console) {
                reader = Some(BufReader::new(f));
            }
        }
        if let Some(r) = reader.as_mut() {
            // Re-seek in case the file was rewritten between polls.
            let _ = r.seek(SeekFrom::Start(last_offset));
            let mut line = String::new();
            loop {
                line.clear();
                match r.read_line(&mut line) {
                    Ok(0) => break,
                    Ok(_) => {
                        last_offset += line.len() as u64;
                        if line.trim_end_matches(['\r', '\n']) == marker {
                            return Ok(());
                        }
                    }
                    Err(_) => break,
                }
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
    /// Wait for `ready_marker` on `console`, then connect + handshake.
    /// Use [`Self::open_uds_only`] when readiness is established elsewhere.
    pub fn open(
        host_uds: &Path,
        guest_port: u32,
        ready_marker: &str,
        console: &Path,
        timeout: Duration,
    ) -> Result<Self, VsockError> {
        watch_ready_marker(console, ready_marker, timeout)?;
        Self::open_uds_only(host_uds, guest_port)
    }

    /// Connect to `host_uds` and hand-shake to `guest_port` without waiting
    /// on a serial-console marker. Used by callers that have already
    /// established readiness through another channel (e.g., a polling
    /// retry loop on the UDS itself).
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

    /// Flush + remove the host-side UDS. NotFound is treated as success so
    /// both `close` and `Drop` paths are idempotent.
    fn teardown(&mut self) -> Result<(), VsockError> {
        self.stream.flush().map_err(VsockError::Io)?;
        match std::fs::remove_file(&self.host_uds) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(VsockError::Io(e)),
        }
    }

    /// Close the connection and remove the host-side UDS. Idempotent.
    pub fn close(mut self) -> Result<(), VsockError> {
        self.closed = true;
        self.teardown()
    }
}

impl Drop for Channel {
    fn drop(&mut self) {
        if !self.closed {
            if let Err(e) = self.teardown() {
                tracing::warn!(path = %self.host_uds.display(), err = %e, "vsock teardown failed during drop");
            }
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
