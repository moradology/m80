//! Host-side bridge between Firecracker UDS and the guest's vsock CID/port.
//!
//! See `README.md` for the black-box contract.
//! Behavior captures: bead epic `m80-xjg` (`br show m80-xjg`).

#![deny(missing_docs)]

mod debug_wire;

use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use sha2::Digest;

use m80_proto::{Envelope, Payload, ProtoError, RawEnvelope};

/// Read/write timeout used only for the Firecracker bridge CONNECT/OK
/// handshake. Application traffic may legitimately stay idle for longer than
/// this; exec duration is enforced by guestd and optional host deadlines.
const BRIDGE_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
/// Maximum socket read timeout used while enforcing an explicit receive
/// deadline. The absolute deadline still owns the final budget.
const DEADLINE_READ_POLL_TIMEOUT: Duration = Duration::from_secs(5);

/// Derive the vsock CID a VM should be assigned from its `vm_id`.
///
/// Pure: no I/O, no allocation table. Two VMs with the same `vm_id` get the
/// same CID; different `vm_id`s get distinct CIDs (modulo the 32-bit space).
/// CID is always in the range `3..=u32::MAX - 1`; Firecracker reserves
/// 0, 1, 2, and `u32::MAX`.
#[must_use]
pub fn cid_for_vm_id(vm_id: &str) -> u32 {
    let mut hasher = sha2::Sha256::new();
    hasher.update(vm_id.as_bytes());
    let bytes = hasher.finalize();
    let raw = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    3 + (raw % (u32::MAX - 3))
}

/// One open connection to the in-VM guestd, bridged via Firecracker's UDS-to-vsock
/// proxy. Created by [`Channel::open_uds_only`], which performs the `CONNECT` /
/// `OK` handshake before returning. Frames are sent with [`Channel::send`] and
/// received with [`Channel::recv`] / [`Channel::recv_raw`]. Normal application
/// reads and writes are blocking; callers that need a host-side read budget use
/// [`Channel::recv_raw_with_deadline`]. An `OversizedPayload` error from `recv`
/// leaves the internal `BufReader` misaligned — the connection is unrecoverable
/// and must be dropped.
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
    T: Payload,
{
    let raw = RawEnvelope::from_typed(envelope);
    if debug_wire::is_enabled("vsock") {
        tracing::trace!(
            direction = "out",
            preview = %debug_wire::format_envelope_preview(&raw)?,
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
            .set_read_timeout(Some(BRIDGE_HANDSHAKE_TIMEOUT))
            .map_err(|e| io_err(&host_uds_arc, e))?;
        stream
            .set_write_timeout(Some(BRIDGE_HANDSHAKE_TIMEOUT))
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
        stream
            .set_read_timeout(None)
            .map_err(|e| io_err(&host_uds_arc, e))?;
        stream
            .set_write_timeout(None)
            .map_err(|e| io_err(&host_uds_arc, e))?;
        buf_reader
            .get_ref()
            .set_read_timeout(None)
            .map_err(|e| io_err(&host_uds_arc, e))?;

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
    ///
    /// # Note — `OversizedPayload` leaves the connection unrecoverable
    ///
    /// If `read_frame` returns [`m80_proto::ProtoError::OversizedPayload`], the
    /// `BufReader` may have partially consumed bytes from the malformed frame.
    /// The internal buffer is now misaligned with respect to the frame boundary,
    /// and subsequent calls to `recv` or `recv_raw` will produce garbage or
    /// further errors. **Callers must not attempt to continue reading after this
    /// error.** Drop the `Channel` and open a new connection. The contract:
    /// `OversizedPayload` is unrecoverable on the same connection.
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
    ///
    /// See [`Channel::recv`] for the `OversizedPayload` unrecoverability
    /// contract — the same applies here.
    pub fn recv_raw(&mut self) -> Result<RawEnvelope, VsockError> {
        let envelope = m80_proto::read_raw_frame(&mut self.buf_reader)?;
        if debug_wire::is_enabled("vsock") {
            tracing::trace!(direction = "in", kind = %envelope.kind, "vsock frame");
        }
        Ok(envelope)
    }

    /// Receive one protobuf frame before `deadline`.
    ///
    /// Returns `Ok(None)` when the deadline expires before the full frame is
    /// read. The method checks the deadline before each underlying socket read,
    /// so a peer cannot hold the call open indefinitely by dripping bytes just
    /// under [`DEADLINE_READ_POLL_TIMEOUT`].
    ///
    /// `Ok(None)` is terminal for this channel: partial frame bytes may already
    /// have been consumed into the frame decoder. Drop the channel instead of
    /// attempting another receive.
    pub fn recv_raw_with_deadline(
        &mut self,
        deadline: Instant,
    ) -> Result<Option<RawEnvelope>, VsockError> {
        let result = {
            let mut reader = DeadlineReader {
                inner: &mut self.buf_reader,
                deadline,
            };
            match m80_proto::read_raw_frame(&mut reader) {
                Ok(envelope) => {
                    if debug_wire::is_enabled("vsock") {
                        tracing::trace!(direction = "in", kind = %envelope.kind, "vsock frame");
                    }
                    Ok(Some(envelope))
                }
                Err(ProtoError::Io(err))
                    if is_read_timeout(err.kind()) && Instant::now() >= deadline =>
                {
                    Ok(None)
                }
                Err(err) => Err(VsockError::Proto(err)),
            }
        };
        if result.is_ok() {
            self.buf_reader
                .get_ref()
                .set_read_timeout(None)
                .map_err(|e| io_err(&self.host_uds, e))?;
        }
        result
    }
}

struct DeadlineReader<'a> {
    inner: &'a mut BufReader<UnixStream>,
    deadline: Instant,
}

impl Read for DeadlineReader<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let Some(timeout) = read_timeout_before(self.deadline) else {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "vsock receive deadline expired",
            ));
        };
        self.inner.get_ref().set_read_timeout(Some(timeout))?;
        match self.inner.read(buf) {
            Ok(read) => Ok(read),
            Err(err) if is_read_timeout(err.kind()) && Instant::now() >= self.deadline => Err(
                io::Error::new(io::ErrorKind::TimedOut, "vsock receive deadline expired"),
            ),
            Err(err) => Err(err),
        }
    }
}

fn read_timeout_before(deadline: Instant) -> Option<Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|remaining| !remaining.is_zero())
        .map(|remaining| remaining.min(DEADLINE_READ_POLL_TIMEOUT))
}

fn is_read_timeout(kind: io::ErrorKind) -> bool {
    matches!(kind, io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock)
}

impl Drop for Channel {
    fn drop(&mut self) {
        // Nothing to flush: stream is a raw UnixStream; the kernel owns the
        // send buffer. Dropping closes the fd and signals EOF to the peer.
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
}

impl Drop for ChannelSender {
    fn drop(&mut self) {
        // Nothing to flush: stream is a raw UnixStream clone; dropping closes
        // the fd. No userspace buffer exists to drain.
    }
}

/// Errors surfaced by [`Channel`] operations.
#[derive(Debug, thiserror::Error)]
pub enum VsockError {
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
    /// Protobuf/framing error once the bridge is established.
    #[error("proto: {0}")]
    Proto(#[from] ProtoError),
}
