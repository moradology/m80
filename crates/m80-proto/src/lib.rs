//! Wire envelope and protobuf framing for m80 host↔guest communication.
//!
//! Both the host (`m80-firecracker`) and in-VM daemon (`m80-guestd`) depend on
//! this crate. Every value that crosses the vsock boundary is wrapped in an
//! [`Envelope`] and serialized as a single length-prefixed protobuf frame.
//!
//! Every application frame carries a protocol `version`; the active host and
//! guest fail closed when that value differs from [`PROTOCOL_VERSION`].
//! The launch ready signal is a separate one-byte
//! boot-readiness check, not an application-channel prelude. `Vec<u8>` fields
//! encode as protobuf `bytes` without base64 inflation; large transfers use
//! bounded chunk frames rather than one unbounded envelope.

#![deny(missing_docs)]

mod error;
mod framing;
mod types;
mod version;
pub mod wire;

pub use error::ProtoError;
pub use framing::{read_frame, read_raw_frame, write_frame, write_raw_frame};
pub use types::{
    CancelRequest, CancelResponse, CancelStatus, DirEntry, DriveDetachRequest, DriveDetachResponse,
    DriveDetachSpec, DriveDetachStatus, DriveDetachStatusKind, DriveHotplugError,
    DriveMountRequest, DriveMountResponse, DriveMountSpec, DriveMountStatus, DriveMountStatusKind,
    Envelope, ExecExit, ExecRequest, ExecResponse, ExecStatus, ExecStderr, ExecStdout, ExecTiming,
    FileError, FileKind, FileListRequest, FileListResponse, FileMkdirRequest, FileMkdirResponse,
    FileReadChunk, FileReadRequest, FileReadResponse, FileRemoveRequest, FileRemoveResponse,
    FileStat, FileStatRequest, FileStatResponse, FileWriteBeginRequest, FileWriteBeginResponse,
    FileWriteChunkRequest, FileWriteChunkResponse, FileWriteCommitRequest, FileWriteCommitResponse,
    FileWriteRequest, FileWriteResponse, GuestCpuMetrics, GuestMemMetrics, HookError, HookHostname,
    HookKindWire, HookResultWire, HookStatus, MetricsRequest, MetricsResponse, Payload,
    PingRequest, PmemMountError, PmemMountRequest, PmemMountResponse, PmemMountSpec,
    PmemMountStatus, PmemMountStatusKind, PongResponse, PostRestoreHookRequest,
    PostRestoreHookResponse, PtyControl, PtyControlEvent, PtyExit, PtyInput, PtyOutput, PtyRequest,
    PtyResize, PtySignal, PtySize, ShutdownAction, ShutdownRequest, ShutdownResponse,
    TenantIdentityReport, FILE_READ_LIMIT_DEFAULT, PAYLOAD_KIND_CANCEL_REQUEST,
    PAYLOAD_KIND_CANCEL_RESPONSE, PAYLOAD_KIND_DRIVE_DETACH_REQUEST,
    PAYLOAD_KIND_DRIVE_DETACH_RESPONSE, PAYLOAD_KIND_DRIVE_MOUNT_REQUEST,
    PAYLOAD_KIND_DRIVE_MOUNT_RESPONSE, PAYLOAD_KIND_EXEC_EXIT, PAYLOAD_KIND_EXEC_REQUEST,
    PAYLOAD_KIND_EXEC_RESPONSE, PAYLOAD_KIND_EXEC_STDERR, PAYLOAD_KIND_EXEC_STDOUT,
    PAYLOAD_KIND_FILE_LIST_REQUEST, PAYLOAD_KIND_FILE_LIST_RESPONSE,
    PAYLOAD_KIND_FILE_MKDIR_REQUEST, PAYLOAD_KIND_FILE_MKDIR_RESPONSE,
    PAYLOAD_KIND_FILE_READ_CHUNK, PAYLOAD_KIND_FILE_READ_REQUEST, PAYLOAD_KIND_FILE_READ_RESPONSE,
    PAYLOAD_KIND_FILE_REMOVE_REQUEST, PAYLOAD_KIND_FILE_REMOVE_RESPONSE,
    PAYLOAD_KIND_FILE_STAT_REQUEST, PAYLOAD_KIND_FILE_STAT_RESPONSE,
    PAYLOAD_KIND_FILE_WRITE_BEGIN_REQUEST, PAYLOAD_KIND_FILE_WRITE_BEGIN_RESPONSE,
    PAYLOAD_KIND_FILE_WRITE_CHUNK_REQUEST, PAYLOAD_KIND_FILE_WRITE_CHUNK_RESPONSE,
    PAYLOAD_KIND_FILE_WRITE_COMMIT_REQUEST, PAYLOAD_KIND_FILE_WRITE_COMMIT_RESPONSE,
    PAYLOAD_KIND_FILE_WRITE_REQUEST, PAYLOAD_KIND_FILE_WRITE_RESPONSE,
    PAYLOAD_KIND_METRICS_REQUEST, PAYLOAD_KIND_METRICS_RESPONSE, PAYLOAD_KIND_PING_REQUEST,
    PAYLOAD_KIND_PMEM_MOUNT_REQUEST, PAYLOAD_KIND_PMEM_MOUNT_RESPONSE, PAYLOAD_KIND_PONG_RESPONSE,
    PAYLOAD_KIND_POST_RESTORE_HOOK_REQUEST, PAYLOAD_KIND_POST_RESTORE_HOOK_RESPONSE,
    PAYLOAD_KIND_PTY_CONTROL, PAYLOAD_KIND_PTY_EXIT, PAYLOAD_KIND_PTY_INPUT,
    PAYLOAD_KIND_PTY_OUTPUT, PAYLOAD_KIND_PTY_REQUEST, PAYLOAD_KIND_PTY_RESIZE,
    PAYLOAD_KIND_SHUTDOWN_REQUEST, PAYLOAD_KIND_SHUTDOWN_RESPONSE,
};
pub use version::{MAX_FRAME_BYTES, PROTOCOL_VERSION};
pub use wire::{encode_raw_envelope, RawEnvelope};

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
