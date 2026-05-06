//! Cleanup behavior vocabulary shared by docs and regression tests.
//!
//! These enums are not a placement state machine. They name the cleanup
//! contract that `m80-firecracker` exposes to callers: stop produces evidence,
//! delete or preserve releases local resources, and higher-level placement
//! authority stays outside this crate.

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
