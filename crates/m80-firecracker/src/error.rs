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
use m80_snapshot::SnapshotError;
use m80_storage::StorageError;
use m80_vsock::VsockError;

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
    /// Snapshot capture or restore failed.
    #[error("snapshot: {0}")]
    Snapshot(#[from] SnapshotError),
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
    Config(String),
    /// The sandbox was idle for longer than `SandboxConfig::idle_timeout`.
    ///
    /// The background watcher has issued a graceful shutdown; the caller must
    /// not send further exec requests. Drop or `stop()` the sandbox to release
    /// resources.
    #[error("sandbox idle timeout expired")]
    IdleTimedOut,
}
