use m80_preflight::{
    HostPrerequisiteCheck, HostPrerequisiteCheckId, HostPrerequisiteFailureKind, PreflightError,
};

#[test]
fn swap_active_maps_devices_to_host_prerequisite_failure() {
    let err = PreflightError::SwapActive {
        devices: vec!["/swapfile".to_owned(), "/dev/zram0".to_owned()],
    };

    let check = HostPrerequisiteCheck::from_preflight_error(&err).unwrap();

    assert_eq!(check.check_id, HostPrerequisiteCheckId::SwapDisabled);
    assert_eq!(
        check.failure_variant,
        Some(HostPrerequisiteFailureKind::SwapActive)
    );
    assert_eq!(check.expected_value.as_deref(), Some("header only"));
    assert_eq!(check.actual_value.as_deref(), Some("/swapfile,/dev/zram0"));
    assert!(err.hint().contains("swapoff -a"));
}
