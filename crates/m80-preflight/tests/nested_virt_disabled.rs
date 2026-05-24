use m80_preflight::{
    HostPrerequisiteCheck, HostPrerequisiteCheckId, HostPrerequisiteFailureKind, PreflightError,
};

#[test]
fn nested_virt_enabled_maps_vendor_to_host_prerequisite_failure() {
    let err = PreflightError::NestedVirtEnabled {
        vendor: "intel".to_owned(),
    };

    let check = HostPrerequisiteCheck::from_preflight_error(&err).unwrap();

    assert_eq!(check.check_id, HostPrerequisiteCheckId::NestedVirtDisabled);
    assert_eq!(
        check.failure_variant,
        Some(HostPrerequisiteFailureKind::NestedVirtEnabled)
    );
    assert_eq!(check.expected_value.as_deref(), Some("N or 0"));
    assert_eq!(check.actual_value.as_deref(), Some("intel nested enabled"));
    assert!(err.hint().contains("M80_SKIP_CHECK_NESTED_VIRT=1"));
}
