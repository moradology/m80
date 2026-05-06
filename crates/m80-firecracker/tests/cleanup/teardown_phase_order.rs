use m80_firecracker::{
    CleanupAuthority, CleanupPhase, CleanupReleaseBlocker, CLEANUP_AUTHORITY, CLEANUP_PHASE_ORDER,
    CLEANUP_RELEASE_BLOCKERS,
};

#[test]
fn phases_run_in_documented_order() {
    assert_eq!(
        CLEANUP_PHASE_ORDER,
        [
            CleanupPhase::AdmissionFence,
            CleanupPhase::BoundedStop,
            CleanupPhase::OptionalChangeExtract,
            CleanupPhase::ResidueCleanup,
            CleanupPhase::Release,
        ]
    );
}

#[test]
fn admission_fence_precedes_destructive_cleanup() {
    assert_eq!(CLEANUP_PHASE_ORDER[0], CleanupPhase::AdmissionFence);
    assert!(
        CLEANUP_PHASE_ORDER
            .iter()
            .position(|phase| *phase == CleanupPhase::ResidueCleanup)
            > CLEANUP_PHASE_ORDER
                .iter()
                .position(|phase| *phase == CleanupPhase::BoundedStop)
    );
}

#[test]
fn generic_block_set() {
    assert_eq!(
        CLEANUP_RELEASE_BLOCKERS,
        [
            CleanupReleaseBlocker::ForcedKillAmbiguous,
            CleanupReleaseBlocker::CleanupFailed,
            CleanupReleaseBlocker::OwnedResidueMayStillBeLive,
        ]
    );
}

#[test]
fn backend_emits_evidence_only() {
    assert_eq!(CLEANUP_AUTHORITY, CleanupAuthority::EvidenceOnly);
}
