use m80_preflight::{
    verify_host_substrate_fixture, CgroupPreflightMode, CheckRow, HostFeaturePreflightConfig,
    HostPrerequisiteCheck, HostPrerequisiteFailureKind, HostPrerequisiteOwner,
    HostPrerequisiteRemediation, HostPrerequisiteResult, HostPrerequisiteResultError,
    HostPrerequisiteStatus, HostSubstrateFixture, PreflightError,
    HOST_PREREQUISITE_RESULT_SCHEMA_VERSION,
};

fn fixture_config() -> HostFeaturePreflightConfig {
    HostFeaturePreflightConfig {
        cgroup_mode: CgroupPreflightMode::UnifiedV2,
        jail_uid: 3000,
        jail_gid: 3000,
        expected_concurrent_vms: 8,
    }
}

#[test]
fn success_rows_reject_failed_table_rows() {
    let rows = [CheckRow {
        label: "legacy table row".to_owned(),
        passed: false,
        detail: "table booleans are not a typed failure contract".to_owned(),
    }];

    let err = HostPrerequisiteResult::from_success_rows(&rows).unwrap_err();

    match err {
        HostPrerequisiteResultError::UnexpectedFailedSuccessRow { check_name } => {
            assert_eq!(check_name, "legacy table row");
        }
        other => panic!("expected unexpected failed success row, got {other:?}"),
    }
}

#[test]
fn valid_result_carries_path_version_hash_mode_owner_and_remediation_shape() {
    let root = HostPrerequisiteOwner { uid: 0, gid: 0 };
    let result = HostPrerequisiteResult::new(vec![
        HostPrerequisiteCheck::pass("firecracker binary")
            .with_final_path("/opt/firecracker/bin/firecracker")
            .with_versions("v1.15.1", "v1.15.1")
            .with_sha256("a".repeat(64), "a".repeat(64))
            .with_modes(0o755, 0o755)
            .with_owners(root, root),
        HostPrerequisiteCheck::fail(
            "kvm",
            HostPrerequisiteFailureKind::KvmUnavailable,
            HostPrerequisiteRemediation::command("enable-kvm", "sudo modprobe kvm"),
        )
        .with_final_path("/dev/kvm"),
    ]);
    let bytes = serde_json::to_vec(&result).unwrap();

    let decoded = HostPrerequisiteResult::from_json_slice(&bytes).unwrap();

    assert_eq!(
        decoded.schema_version,
        HOST_PREREQUISITE_RESULT_SCHEMA_VERSION
    );
    assert_eq!(
        decoded.checks[0].final_path.as_deref().unwrap(),
        std::path::Path::new("/opt/firecracker/bin/firecracker")
    );
    assert_eq!(
        decoded.checks[0].expected_version.as_deref(),
        Some("v1.15.1")
    );
    assert_eq!(
        decoded.checks[0].actual_sha256.as_deref(),
        Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
    );
    assert_eq!(decoded.checks[0].expected_mode, Some(0o755));
    assert_eq!(decoded.checks[0].expected_owner, Some(root));
    assert_eq!(decoded.checks[1].status, HostPrerequisiteStatus::Fail);
    assert_eq!(
        decoded.checks[1].failure_variant,
        Some(HostPrerequisiteFailureKind::KvmUnavailable)
    );
    assert_eq!(
        decoded.checks[1].remediation.as_ref().unwrap().id,
        "enable-kvm"
    );
}

#[test]
fn stale_schema_version_fails_closed() {
    let raw = br#"{
        "schema_version": 99,
        "checks": []
    }"#;

    let err = HostPrerequisiteResult::from_json_slice(raw).unwrap_err();

    match err {
        HostPrerequisiteResultError::UnsupportedSchemaVersion { expected, actual } => {
            assert_eq!(expected, HOST_PREREQUISITE_RESULT_SCHEMA_VERSION);
            assert_eq!(actual, 99);
        }
        other => panic!("expected unsupported schema version, got {other:?}"),
    }
}

#[test]
fn missing_required_field_fails_closed() {
    let raw = br#"{
        "schema_version": 1,
        "checks": [
            { "status": "pass" }
        ]
    }"#;

    let err = HostPrerequisiteResult::from_json_slice(raw).unwrap_err();

    assert!(matches!(err, HostPrerequisiteResultError::Json(_)));
}

#[test]
fn unknown_failure_variant_fails_closed() {
    let raw = br#"{
        "schema_version": 1,
        "checks": [
            {
                "check_name": "kvm",
                "status": "fail",
                "failure_variant": "future_failure",
                "remediation": {
                    "id": "enable-kvm",
                    "policy_link": "docs/ops/host-setup.md"
                }
            }
        ]
    }"#;

    let err = HostPrerequisiteResult::from_json_slice(raw).unwrap_err();

    assert!(matches!(err, HostPrerequisiteResultError::Json(_)));
}

#[test]
fn missing_failure_variant_fails_closed() {
    let raw = br#"{
        "schema_version": 1,
        "checks": [
            {
                "check_name": "kvm",
                "status": "fail",
                "remediation": {
                    "id": "enable-kvm",
                    "policy_link": "docs/ops/host-setup.md"
                }
            }
        ]
    }"#;

    let err = HostPrerequisiteResult::from_json_slice(raw).unwrap_err();

    match err {
        HostPrerequisiteResultError::MissingFailureVariant { check_name } => {
            assert_eq!(check_name, "kvm");
        }
        other => panic!("expected missing failure variant, got {other:?}"),
    }
}

#[test]
fn missing_remediation_token_fails_closed() {
    let raw = br#"{
        "schema_version": 1,
        "checks": [
            {
                "check_name": "kvm",
                "status": "fail",
                "failure_variant": "kvm_unavailable",
                "remediation": {
                    "id": "",
                    "policy_link": "docs/ops/host-setup.md"
                }
            }
        ]
    }"#;

    let err = HostPrerequisiteResult::from_json_slice(raw).unwrap_err();

    match err {
        HostPrerequisiteResultError::MissingRemediationToken { check_name } => {
            assert_eq!(check_name, "kvm");
        }
        other => panic!("expected missing remediation token, got {other:?}"),
    }
}

#[test]
fn missing_remediation_target_fails_closed() {
    let raw = br#"{
        "schema_version": 1,
        "checks": [
            {
                "check_name": "kvm",
                "status": "fail",
                "failure_variant": "kvm_unavailable",
                "remediation": {
                    "id": "enable-kvm"
                }
            }
        ]
    }"#;

    let err = HostPrerequisiteResult::from_json_slice(raw).unwrap_err();

    match err {
        HostPrerequisiteResultError::MissingRemediationTarget { check_name } => {
            assert_eq!(check_name, "kvm");
        }
        other => panic!("expected missing remediation target, got {other:?}"),
    }
}

#[test]
fn failure_kind_maps_host_feature_preflight_errors() {
    let cases = [
        (
            PreflightError::KvmCpuExtensionMissing,
            HostPrerequisiteFailureKind::KvmCpuExtensionMissing,
        ),
        (
            PreflightError::CpuVulnerabilityDetected {
                id: "mds".to_owned(),
                detail: "Vulnerable".to_owned(),
            },
            HostPrerequisiteFailureKind::CpuVulnerabilityDetected,
        ),
        (
            PreflightError::VsockUnavailable,
            HostPrerequisiteFailureKind::VsockUnavailable,
        ),
        (
            PreflightError::TunUnavailable,
            HostPrerequisiteFailureKind::TunUnavailable,
        ),
        (
            PreflightError::NfConntrackUnavailable,
            HostPrerequisiteFailureKind::NfConntrackUnavailable,
        ),
        (
            PreflightError::BridgeNetfilterUnavailable,
            HostPrerequisiteFailureKind::BridgeNetfilterUnavailable,
        ),
        (
            PreflightError::BridgeNfCallIptablesDisabled {
                actual: "0".to_owned(),
            },
            HostPrerequisiteFailureKind::BridgeNfCallIptablesDisabled,
        ),
        (
            PreflightError::NfConntrackCapacityTooLow {
                actual: 128,
                minimum: 1024,
                expected_concurrent_vms: 8,
            },
            HostPrerequisiteFailureKind::NfConntrackCapacityTooLow,
        ),
        (
            PreflightError::InvalidNfConntrackMax {
                actual: "nope".to_owned(),
            },
            HostPrerequisiteFailureKind::InvalidNfConntrackMax,
        ),
        (
            PreflightError::InvalidExpectedConcurrentVms {
                actual: "0".to_owned(),
            },
            HostPrerequisiteFailureKind::InvalidExpectedConcurrentVms,
        ),
        (
            PreflightError::KernelModulesMissing {
                missing: vec!["tap".to_owned()],
            },
            HostPrerequisiteFailureKind::KernelModulesMissing,
        ),
    ];

    for (error, expected) in cases {
        assert_eq!(
            HostPrerequisiteFailureKind::from_preflight_error(&error),
            Some(expected)
        );
    }
}

#[test]
fn hostless_substrate_verifier_emits_result_contract() {
    let discovery =
        verify_host_substrate_fixture(fixture_config(), &HostSubstrateFixture::supported_root())
            .unwrap();

    assert_eq!(
        discovery.host_prerequisites.schema_version,
        HOST_PREREQUISITE_RESULT_SCHEMA_VERSION
    );
    assert!(discovery
        .host_prerequisites
        .checks
        .iter()
        .any(|check| check.check_name == "Host substrate proof"
            && check.status == HostPrerequisiteStatus::Pass));
}
