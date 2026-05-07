//! Wire envelope and NDJSON framing for m80 host↔guest communication.
//!
//! Both the host (`m80-firecracker`) and in-VM daemon (`m80-guestd`) depend on
//! this crate. Every value that crosses the vsock boundary is wrapped in an
//! [`Envelope`] and serialized as a single NDJSON record.
//!
//! Peers exchange a [`HandshakeMessage`] on every fresh connection before
//! processing any application payload. [`negotiate_version`] validates the
//! remote version. `Vec<u8>` fields encode as base64 (~33% inflation), so the
//! practical raw-output budget per [`MAX_FRAME_BYTES`]-capped frame is around
//! 2.9 MiB. The `kind` discriminator on [`Envelope`] reserves the extension
//! point for v0.2+ payload types without bumping [`PROTOCOL_VERSION`].

#![deny(missing_docs)]

mod error;
mod framing;
mod types;
mod version;
#[cfg(test)]
pub(crate) mod test_helpers;

pub use error::ProtoError;
pub use framing::{read_frame, write_frame};
pub use types::{
    CancelAck, CancelRequest, CancelStatus, DirEntry, Envelope, ExecExit, ExecRequest,
    ExecResponse, ExecStatus, ExecStderr, ExecStdout, ExecTiming, FileError, FileKind,
    FileListRequest, FileListResponse, FileReadRequest, FileReadResponse, FileRemoveRequest,
    FileRemoveResponse, FileStat, FileStatRequest, FileStatResponse, FileWriteBeginRequest,
    FileWriteBeginResponse, FileWriteChunkRequest, FileWriteChunkResponse, FileWriteCommitRequest,
    FileWriteCommitResponse, FileWriteRequest, FileWriteResponse, GuestCpuMetrics, GuestMemMetrics,
    HandshakeMessage, MetricsRequest, MetricsResponse, Payload, PtyControl, PtyControlEvent,
    PtyExit, PtyInput, PtyOutput, PtyRequest, PtyResize, PtySignal, PtySize, ShutdownAction,
    ShutdownRequest, ShutdownResponse, FILE_READ_LIMIT_DEFAULT, PAYLOAD_KIND_CANCEL_ACK,
    PAYLOAD_KIND_CANCEL_REQUEST, PAYLOAD_KIND_EXEC_EXIT, PAYLOAD_KIND_EXEC_REQUEST,
    PAYLOAD_KIND_EXEC_RESPONSE, PAYLOAD_KIND_EXEC_STDERR, PAYLOAD_KIND_EXEC_STDOUT,
    PAYLOAD_KIND_FILE_LIST_REQUEST, PAYLOAD_KIND_FILE_LIST_RESPONSE,
    PAYLOAD_KIND_FILE_READ_REQUEST, PAYLOAD_KIND_FILE_READ_RESPONSE,
    PAYLOAD_KIND_FILE_REMOVE_REQUEST, PAYLOAD_KIND_FILE_REMOVE_RESPONSE,
    PAYLOAD_KIND_FILE_STAT_REQUEST, PAYLOAD_KIND_FILE_STAT_RESPONSE,
    PAYLOAD_KIND_FILE_WRITE_BEGIN_REQUEST, PAYLOAD_KIND_FILE_WRITE_BEGIN_RESPONSE,
    PAYLOAD_KIND_FILE_WRITE_CHUNK_REQUEST, PAYLOAD_KIND_FILE_WRITE_CHUNK_RESPONSE,
    PAYLOAD_KIND_FILE_WRITE_COMMIT_REQUEST, PAYLOAD_KIND_FILE_WRITE_COMMIT_RESPONSE,
    PAYLOAD_KIND_FILE_WRITE_REQUEST, PAYLOAD_KIND_FILE_WRITE_RESPONSE,
    PAYLOAD_KIND_METRICS_REQUEST, PAYLOAD_KIND_METRICS_RESPONSE, PAYLOAD_KIND_PTY_CONTROL,
    PAYLOAD_KIND_PTY_EXIT, PAYLOAD_KIND_PTY_INPUT, PAYLOAD_KIND_PTY_OUTPUT,
    PAYLOAD_KIND_PTY_REQUEST, PAYLOAD_KIND_PTY_RESIZE, PAYLOAD_KIND_SHUTDOWN_REQUEST,
    PAYLOAD_KIND_SHUTDOWN_RESPONSE,
};
pub use version::{negotiate_version, MAX_FRAME_BYTES, PROTOCOL_VERSION};

/// Default vsock port the in-VM `m80-guestd` daemon listens on.
///
/// Both the host (via `m80-vsock`/`m80-firecracker`) and the guest daemon
/// must agree on this value; it is the single source of truth for the
/// host↔guest port convention.
pub const GUEST_PORT_DEFAULT: u32 = 9001;

/// Default vsock port for the inverted-readiness signal: m80-guestd
/// connects out to the host on this port immediately after binding its
/// own [`GUEST_PORT_DEFAULT`] listener. The host pre-creates a
/// `UnixListener` at `<vsock_uds>_<READY_PORT_DEFAULT>` (matching
/// Firecracker's muxer convention; see
/// `firecracker/src/vmm/src/devices/virtio/vsock/unix/muxer.rs:619-641`)
/// and `accept()`s — event-driven readiness without polling, eliminating
/// the EAGAIN race in the muxer's accept loop that polled CONNECT/OK
/// provoked.
///
/// The number itself (52525) is chosen to be visually distinct from
/// common dev-server TCP ports (3000, 5000, 8000, 8080, 9000, etc.) so
/// stack traces, logs, and `lsof` output don't mislead a reader into
/// thinking m80 collides with their dev environment. vsock ports are a
/// separate addressing space from TCP/UDP, so there is no actual
/// conflict — the convention is purely about reader ergonomics.
pub const READY_PORT_DEFAULT: u32 = 52525;

/// Default legacy ready-marker that `m80-guestd` may print to the serial
/// console once it is listening on vsock.
///
/// m80 launch readiness does not watch this marker. The host's load-bearing
/// readiness contract is the inverted-ready connection on
/// [`READY_PORT_DEFAULT`]. This value remains centralized for images and logs
/// that still want the familiar guest-side marker string.
pub const READY_MARKER_DEFAULT: &str = "GUESTD_READY";
