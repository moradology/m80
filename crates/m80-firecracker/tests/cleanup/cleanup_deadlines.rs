use std::time::Duration;

use m80_firecracker::{CleanupDeadlinePhase, FcError, FcErrorKind};

#[test]
fn cleanup_deadline_error_is_a_stable_transient_timeout_contract() {
    let err = FcError::CleanupDeadlineExceeded {
        vm_id: "vm-cleanup-deadline".to_owned(),
        phase: CleanupDeadlinePhase::RunDirDelete,
        timeout: Duration::from_secs(5),
    };

    assert_eq!(err.variant_name(), "CleanupDeadlineExceeded");
    assert_eq!(err.kind(), FcErrorKind::Transient);
    assert_eq!(CleanupDeadlinePhase::JailDrop.to_string(), "jail_drop");
    assert_eq!(
        CleanupDeadlinePhase::NetworkCleanup.to_string(),
        "network_cleanup"
    );
    assert_eq!(
        CleanupDeadlinePhase::RunDirDelete.to_string(),
        "run_dir_delete"
    );
}
