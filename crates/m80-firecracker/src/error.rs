//! [`FcError`] — top-level error sum for `m80-firecracker`.

use std::io;
use std::path::PathBuf;
use std::time::Duration;

use m80_cgroup::CgroupError;
use m80_firecracker_client::ClientError;
use m80_image_manifest::ManifestError;
use m80_jailer::JailerError;
use m80_net_outbound::NetError;
use m80_preflight::PreflightError;
use m80_proto::{DriveHotplugError, FileError};
use m80_snapshot::SnapshotError;
use m80_storage::StorageError;
use m80_vsock::VsockError;

/// Host-side wire protocol failures after a vsock channel is open.
#[derive(Debug, thiserror::Error)]
pub enum WireProtocolError {
    /// Peer sent bytes that could not be decoded as an m80 protobuf envelope.
    #[error("malformed peer frame: {0}")]
    MalformedPeer(String),
    /// Peer announced or encoded a protobuf frame larger than the active cap.
    #[error("oversized frame: {size} bytes exceeds limit of {limit}")]
    OversizedFrame {
        /// Observed frame size in bytes.
        size: usize,
        /// Configured frame cap in bytes.
        limit: usize,
    },
    /// Peer used a protocol version this binary does not speak.
    #[error("unsupported protocol version: expected {expected}, got {got}")]
    UnsupportedVersion {
        /// Version expected by this binary.
        expected: u32,
        /// Version observed on the wire.
        got: u32,
    },
    /// Peer sent a well-formed frame whose kind is illegal in the current state.
    #[error("unexpected frame in {context}: expected {expected}, got {got}")]
    UnexpectedFrame {
        /// State or request that was receiving the frame.
        context: &'static str,
        /// Expected frame kind or set.
        expected: &'static str,
        /// Actual frame kind.
        got: String,
    },
    /// Peer disconnected before the request produced its required terminal frame.
    #[error("disconnect before terminal frame in {context}")]
    DisconnectBeforeTerminal {
        /// State or request that was awaiting a terminal frame.
        context: &'static str,
    },
    /// Peer sent a stream chunk out of sequence.
    #[error("stream sequence mismatch in {stream}: expected {expected}, got {got}")]
    SequenceMismatch {
        /// Stream whose sequence was violated.
        stream: &'static str,
        /// Expected sequence number.
        expected: u64,
        /// Actual sequence number.
        got: u64,
    },
}

/// Bounded lifecycle failure vocabulary used in behavior docs and tests.
///
/// The top-level [`FcError`] variants still preserve the concrete source
/// error. This enum gives operators and future machine readers a stable
/// vocabulary for lifecycle cleanup/readiness classes without reintroducing
/// predecessor agent semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleFailureKind {
    /// Guest daemon did not become ready in the launch/restore budget.
    GuestdNotReady,
    /// Vsock transport failed after readiness should have been established.
    BrokenVsock,
    /// The VM did not respond to a lifecycle operation within its budget.
    StuckVm,
    /// Guest graceful-stop RPC did not complete before its deadline.
    GracefulStopTimeout,
    /// Host forced a Firecracker kill after a graceful path failed or was skipped.
    ForcedKillFallback,
    /// Host-side resource cleanup failed.
    CleanupFailure,
    /// Workspace writeback was intentionally skipped after an unclean stop.
    WritebackSkippedAfterUncleanStop,
}

impl LifecycleFailureKind {
    /// Stable complete list. Adding a variant is a public behavior change.
    pub const ALL: [Self; 7] = [
        Self::GuestdNotReady,
        Self::BrokenVsock,
        Self::StuckVm,
        Self::GracefulStopTimeout,
        Self::ForcedKillFallback,
        Self::CleanupFailure,
        Self::WritebackSkippedAfterUncleanStop,
    ];
}

/// Structured configuration error. Used as the inner payload of
/// [`FcError::Config`].
///
/// `Other` is a last-resort fallback for anyhow chains at binary edges.
/// Prefer `TomlSyntax`, `MissingField`, or `InvalidValue` when the site
/// has enough information.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// A TOML config file could not be parsed.
    #[error("{}: {source}", path.display())]
    TomlSyntax {
        /// Path of the file that failed to parse.
        path: PathBuf,
        /// The parse error from `toml`.
        source: toml::de::Error,
    },
    /// A required configuration field was absent.
    #[error("missing required field `{field}`")]
    MissingField {
        /// Name of the missing field.
        field: &'static str,
    },
    /// A field was present but its value was rejected.
    #[error("invalid value for `{field}`: {reason}")]
    InvalidValue {
        /// Name of the invalid field.
        field: &'static str,
        /// Human-readable rejection reason.
        reason: String,
    },
    /// Catch-all for anyhow chains at binary edges. Document the specific
    /// call site with a comment; prefer a typed variant where possible.
    #[error("{0}")]
    Other(String),
}

/// Top-level error for `m80-firecracker`. Each variant tells the caller
/// which phase failed; the inner cause carries phase-specific detail.
#[derive(Debug, thiserror::Error)]
pub enum FcError {
    /// Preflight check failed.
    #[error("preflight: {0}")]
    Preflight(#[from] PreflightError),
    /// Manifest read/validate failed.
    #[error("manifest: {0}")]
    Manifest(#[from] ManifestError),
    /// Storage operation failed.
    #[error("storage: {0}")]
    Storage(#[from] StorageError),
    /// Jailer materialization or recovery failed.
    #[error("jailer: {0}")]
    Jailer(#[from] JailerError),
    /// Cgroup subtree create / apply_limits / cleanup failed. Distinct
    /// from `Config` so callers can branch on "kernel/cgroup setup
    /// failed" vs "host config didn't parse".
    #[error("cgroup: {0}")]
    Cgroup(#[from] CgroupError),
    /// Network realization or cleanup failed.
    #[error("network: {0}")]
    Network(#[from] NetError),
    /// Firecracker REST API call failed.
    #[error("client: {0}")]
    Client(#[from] ClientError),
    /// Vsock channel operation failed. Wire-protocol errors arrive here as
    /// `VsockError::Proto(...)` since vsock is the only transport that
    /// speaks `m80-proto` envelopes in this crate; there is no separate
    /// `Proto` variant.
    #[error("vsock: {0}")]
    Vsock(#[from] VsockError),
    /// Host-side protocol failure after a vsock channel was established.
    #[error("protocol: {0}")]
    Protocol(WireProtocolError),
    /// Snapshot capture or restore failed.
    #[error("snapshot: {0}")]
    Snapshot(#[from] SnapshotError),
    /// Guest-side file operation failed with a typed wire error.
    #[error("file operation: {0:?}")]
    FileOp(FileError),
    /// Guest-side drive hotplug failed with a typed wire error.
    #[error("drive hotplug: {0:?}")]
    DriveHotplug(DriveHotplugError),
    /// Guest-mounted drive identity bytes did not match the caller's expected
    /// opaque identity.
    #[error(
        "tenant identity mismatch for {drive_id}: expected {expected_len} bytes, got {actual_len}"
    )]
    TenantIdentityMismatch {
        /// Firecracker drive id whose mounted identity was checked.
        drive_id: String,
        /// Expected opaque identity byte length.
        expected_len: usize,
        /// Actual opaque identity byte length.
        actual_len: usize,
    },
    /// Admission was refused (semaphore at limit; no permit available).
    #[error("admission refused: {limit} concurrent VMs already running")]
    AdmissionRefused {
        /// Configured admission limit.
        limit: u32,
    },
    /// A warm pool allocation was requested while no ready slots were
    /// available. The allocator never hides this with a cold-boot fallback.
    #[error("warm pool empty: 0 ready slots available for target {target_ready}")]
    PoolEmpty {
        /// Configured ready-slot target.
        target_ready: usize,
    },
    /// The lifecycle state machine was in an unexpected state.
    #[error("invalid lifecycle state: expected {expected}, got {actual}")]
    InvalidState {
        /// State the operation expected.
        expected: &'static str,
        /// State the sandbox was actually in.
        actual: &'static str,
    },
    /// Firecracker did not expose its REST API socket within the launch
    /// budget.
    #[error(
        "Firecracker API socket {} did not appear within {timeout:?}",
        path.display()
    )]
    ApiSocketTimeout {
        /// Host path to the API socket m80 waited for.
        path: PathBuf,
        /// Launch budget that expired.
        timeout: Duration,
    },
    /// m80-guestd did not connect on the inverted-readiness socket within
    /// the launch/restore budget.
    #[error(
        "guestd ready signal {} did not arrive within {timeout:?}",
        path.display()
    )]
    GuestdReadyTimeout {
        /// Host-side readiness or vsock path m80 waited on.
        path: PathBuf,
        /// Readiness budget that expired.
        timeout: Duration,
    },
    /// Underlying I/O failure.
    #[error("i/o: {0}")]
    Io(#[from] io::Error),
    /// Configuration loading or merging failure.
    #[error("config: {0}")]
    Config(ConfigError),
    /// The sandbox was idle for longer than `SandboxConfig::idle_timeout`.
    ///
    /// The background watcher has issued a graceful shutdown; the caller must
    /// not send further exec requests. Drop or `stop()` the sandbox to release
    /// resources.
    #[error("sandbox idle timeout expired")]
    IdleTimedOut,
}

impl FcError {
    /// Convenience constructor for `FcError::Config(ConfigError::Other(msg))`.
    ///
    /// Use at call sites that have a formatted string but no more-specific
    /// `ConfigError` variant. Prefer `InvalidValue` or `MissingField` when
    /// the site has enough information.
    pub fn config_other(msg: impl Into<String>) -> Self {
        FcError::Config(ConfigError::Other(msg.into()))
    }
}

/// Ordered host-side teardown phases for a running VM.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CleanupPhase {
    /// Stop accepting new work on this handle.
    AdmissionFence,
    /// Bound the guest shutdown attempt and host kill step.
    BoundedStop,
    /// Optional caller-requested scratch extraction after stop.
    OptionalChangeExtract,
    /// Drop or clean owned host resources.
    ResidueCleanup,
    /// Release the admission permit by deleting or preserving the stopped run-dir.
    Release,
}

/// Current v0.1 teardown phase order.
pub const CLEANUP_PHASE_ORDER: [CleanupPhase; 5] = [
    CleanupPhase::AdmissionFence,
    CleanupPhase::BoundedStop,
    CleanupPhase::OptionalChangeExtract,
    CleanupPhase::ResidueCleanup,
    CleanupPhase::Release,
];

/// Observable stop paths exposed by `m80-firecracker`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopDisposition {
    /// Ask m80-guestd to shut down, then kill the Firecracker pid.
    GuestdShutdownThenFirecrackerKill,
    /// Skip the guest request and kill Firecracker plus jailer pids.
    HostForceKill,
}

/// Current v0.1 stop dispositions.
pub const STOP_DISPOSITIONS: [StopDisposition; 2] = [
    StopDisposition::GuestdShutdownThenFirecrackerKill,
    StopDisposition::HostForceKill,
];

/// Generic conditions that prevent a caller from treating cleanup as releasable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CleanupReleaseBlocker {
    /// The host cannot prove the forced kill completed cleanly.
    ForcedKillAmbiguous,
    /// Owned host cleanup returned an error.
    CleanupFailed,
    /// Owned residue may still represent a live VM.
    OwnedResidueMayStillBeLive,
}

/// Generic release blockers owned by m80's VM mechanics.
pub const CLEANUP_RELEASE_BLOCKERS: [CleanupReleaseBlocker; 3] = [
    CleanupReleaseBlocker::ForcedKillAmbiguous,
    CleanupReleaseBlocker::CleanupFailed,
    CleanupReleaseBlocker::OwnedResidueMayStillBeLive,
];

/// Authority boundary for cleanup decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CleanupAuthority {
    /// m80 emits local lifecycle evidence but does not advance placement state.
    EvidenceOnly,
}

/// m80-firecracker's cleanup authority mode.
pub const CLEANUP_AUTHORITY: CleanupAuthority = CleanupAuthority::EvidenceOnly;
