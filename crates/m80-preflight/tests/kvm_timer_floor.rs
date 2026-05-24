use m80_preflight::{
    HostPrerequisiteCheck, HostPrerequisiteCheckId, HostPrerequisiteFailureKind, PreflightError,
};

#[test]
fn kvm_timer_floor_unset_maps_to_host_prerequisite_failure() {
    let err = PreflightError::KvmTimerFloorUnset {
        actual: "0".to_owned(),
    };

    let check = HostPrerequisiteCheck::from_preflight_error(&err).unwrap();

    assert_eq!(check.check_id, HostPrerequisiteCheckId::KvmTimerFloor);
    assert_eq!(
        check.failure_variant,
        Some(HostPrerequisiteFailureKind::KvmTimerFloorUnset)
    );
    assert_eq!(check.expected_value.as_deref(), Some(">= 500"));
    assert_eq!(check.actual_value.as_deref(), Some("0"));
    assert!(err.hint().contains("M80_SKIP_CHECK_KVM_TIMER=1"));
}
