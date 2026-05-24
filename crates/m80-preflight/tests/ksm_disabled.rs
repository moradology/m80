use m80_preflight::{
    HostPrerequisiteCheck, HostPrerequisiteCheckId, HostPrerequisiteFailureKind, PreflightError,
};

#[test]
fn ksm_enabled_maps_to_host_prerequisite_failure() {
    let err = PreflightError::KsmEnabled {
        actual: "1".to_owned(),
    };

    let check = HostPrerequisiteCheck::from_preflight_error(&err).unwrap();

    assert_eq!(check.check_id, HostPrerequisiteCheckId::KsmDisabled);
    assert_eq!(
        check.failure_variant,
        Some(HostPrerequisiteFailureKind::KsmEnabled)
    );
    assert_eq!(check.expected_value.as_deref(), Some("0"));
    assert_eq!(check.actual_value.as_deref(), Some("1"));
    assert!(err
        .hint()
        .contains("echo 0 | sudo tee /sys/kernel/mm/ksm/run"));
    assert!(err.hint().contains("M80_SKIP_CHECK_KSM=1"));
}
