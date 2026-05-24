use m80_preflight::{
    HostPrerequisiteCheck, HostPrerequisiteCheckId, HostPrerequisiteFailureKind, PreflightError,
};

#[test]
fn smt_enabled_hard_fail_maps_to_host_prerequisite_failure() {
    let err = PreflightError::SmtEnabled {
        actual: "on".to_owned(),
    };

    let check = HostPrerequisiteCheck::from_preflight_error(&err).unwrap();

    assert_eq!(check.check_id, HostPrerequisiteCheckId::SmtDisabled);
    assert_eq!(
        check.failure_variant,
        Some(HostPrerequisiteFailureKind::SmtEnabled)
    );
    assert_eq!(check.expected_value.as_deref(), Some("off"));
    assert_eq!(check.actual_value.as_deref(), Some("on"));
    assert!(err.hint().contains("M80_SMT_CHECK"));
}
