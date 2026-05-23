use std::path::Path;

use m80_preflight::{
    verify_host_substrate_fixture, CgroupPreflightMode, CheckRow, HostFeaturePreflightConfig,
    HostPrerequisiteCheck, HostPrerequisiteCheckId, HostPrerequisiteFailureKind,
    HostPrerequisiteOwner, HostPrerequisiteRemediation, HostPrerequisiteResult,
    HostPrerequisiteResultError, HostPrerequisiteStatus, HostSubstrateFixture, PreflightError,
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

fn full_report_rows() -> Vec<CheckRow> {
    HostPrerequisiteCheckId::ALL
        .iter()
        .copied()
        .map(|check_id| CheckRow::pass(check_id, "fixture"))
        .collect()
}

#[test]
fn success_rows_reject_failed_table_rows() {
    let rows = [CheckRow::fail(
        HostPrerequisiteCheckId::Kvm,
        "table booleans are not a typed failure contract",
    )
    .with_label("legacy table row")];

    let err = HostPrerequisiteResult::from_success_rows(&rows).unwrap_err();

    match err {
        HostPrerequisiteResultError::UnexpectedFailedSuccessRow { check_name } => {
            assert_eq!(check_name, "legacy table row");
        }
        other => panic!("expected unexpected failed success row, got {other:?}"),
    }
}

#[test]
fn success_rows_preserve_check_id_when_human_label_changes() {
    let rows = [CheckRow::pass(HostPrerequisiteCheckId::Kvm, "present")
        .with_label("Kernel virtualization device")];

    let result = HostPrerequisiteResult::from_success_rows(&rows).unwrap();

    assert_eq!(result.checks[0].check_id, HostPrerequisiteCheckId::Kvm);
    assert_eq!(result.checks[0].check_name, "Kernel virtualization device");
}

#[test]
fn full_report_rows_keep_registry_order_despite_human_label_changes() {
    let rows = full_report_rows()
        .into_iter()
        .map(|row| {
            let check_id = row.check_id;
            row.with_label(format!("human label for {}", check_id.as_str()))
        })
        .collect::<Vec<_>>();

    let result = HostPrerequisiteResult::from_full_report_rows(&rows).unwrap();
    let check_ids = result
        .checks
        .iter()
        .map(|check| check.check_id)
        .collect::<Vec<_>>();

    assert_eq!(check_ids, HostPrerequisiteCheckId::ALL);
    assert_eq!(
        result.checks[0].check_name, "human label for os_gate",
        "human label changes must not affect the machine check-id order"
    );
}

#[test]
fn full_report_rows_name_first_missing_check_id() {
    let rows = full_report_rows()
        .into_iter()
        .filter(|row| row.check_id != HostPrerequisiteCheckId::RootfsManifest)
        .collect::<Vec<_>>();

    let err = HostPrerequisiteResult::from_full_report_rows(&rows).unwrap_err();

    match err {
        HostPrerequisiteResultError::MissingCheckId {
            check_id,
            expected_index,
        } => {
            assert_eq!(check_id, "rootfs_manifest");
            assert_eq!(expected_index, 22);
        }
        other => panic!("expected missing check_id, got {other:?}"),
    }
}

#[test]
fn full_report_rows_name_first_duplicate_check_id() {
    let mut rows = full_report_rows();
    rows.insert(
        4,
        CheckRow::pass(HostPrerequisiteCheckId::Kvm, "duplicate fixture"),
    );

    let err = HostPrerequisiteResult::from_full_report_rows(&rows).unwrap_err();

    match err {
        HostPrerequisiteResultError::DuplicateCheckId {
            check_id,
            first_index,
            duplicate_index,
        } => {
            assert_eq!(check_id, "kvm");
            assert_eq!(first_index, 2);
            assert_eq!(duplicate_index, 4);
        }
        other => panic!("expected duplicate check_id, got {other:?}"),
    }
}

#[test]
fn full_report_rows_name_first_out_of_order_check_id() {
    let mut rows = full_report_rows();
    rows.swap(21, 22);

    let err = HostPrerequisiteResult::from_full_report_rows(&rows).unwrap_err();

    match err {
        HostPrerequisiteResultError::OutOfOrderCheckId {
            expected_check_id,
            actual_check_id,
            index,
        } => {
            assert_eq!(expected_check_id, "kernel_image");
            assert_eq!(actual_check_id, "rootfs_manifest");
            assert_eq!(index, 21);
        }
        other => panic!("expected out-of-order check_id, got {other:?}"),
    }
}

#[test]
fn stable_check_registry_keeps_expected_order() {
    assert_eq!(
        HostPrerequisiteCheckId::ALL,
        &[
            HostPrerequisiteCheckId::OsGate,
            HostPrerequisiteCheckId::HostKernelFloor,
            HostPrerequisiteCheckId::Kvm,
            HostPrerequisiteCheckId::CgroupMode,
            HostPrerequisiteCheckId::JailerIdentity,
            HostPrerequisiteCheckId::Privilege,
            HostPrerequisiteCheckId::HostSubstrateProof,
            HostPrerequisiteCheckId::KvmCpuExtensions,
            HostPrerequisiteCheckId::KernelModules,
            HostPrerequisiteCheckId::TransparentHugepages,
            HostPrerequisiteCheckId::KvmHaltPolling,
            HostPrerequisiteCheckId::CpuGovernor,
            HostPrerequisiteCheckId::CpuMicrocode,
            HostPrerequisiteCheckId::CpuVulnerabilities,
            HostPrerequisiteCheckId::ConntrackCapacity,
            HostPrerequisiteCheckId::FirecrackerBinary,
            HostPrerequisiteCheckId::FirecrackerSeccompFilter,
            HostPrerequisiteCheckId::JailerBinary,
            HostPrerequisiteCheckId::JailerHardeningWrapper,
            HostPrerequisiteCheckId::NetworkHelper,
            HostPrerequisiteCheckId::HostBinaryManifest,
            HostPrerequisiteCheckId::KernelImage,
            HostPrerequisiteCheckId::RootfsManifest,
            HostPrerequisiteCheckId::RunRoot,
            HostPrerequisiteCheckId::RunRootFilesystem,
            HostPrerequisiteCheckId::StorageHelpers,
        ]
    );
}

#[test]
fn valid_result_carries_path_version_hash_mode_owner_and_remediation_shape() {
    let root = HostPrerequisiteOwner { uid: 0, gid: 0 };
    let result = HostPrerequisiteResult::new(vec![
        HostPrerequisiteCheck::pass(HostPrerequisiteCheckId::FirecrackerBinary)
            .with_final_path("/opt/firecracker/bin/firecracker")
            .with_values("present", "present")
            .with_versions("v1.15.1", "v1.15.1")
            .with_sha256("a".repeat(64), "a".repeat(64))
            .with_modes(0o755, 0o755)
            .with_owners(root, root),
        HostPrerequisiteCheck::fail(
            HostPrerequisiteCheckId::Kvm,
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
        decoded.checks[0].check_id,
        HostPrerequisiteCheckId::FirecrackerBinary
    );
    assert_eq!(
        decoded.checks[0].final_path.as_deref().unwrap(),
        std::path::Path::new("/opt/firecracker/bin/firecracker")
    );
    assert_eq!(
        decoded.checks[0].expected_version.as_deref(),
        Some("v1.15.1")
    );
    assert_eq!(decoded.checks[0].expected_value.as_deref(), Some("present"));
    assert_eq!(decoded.checks[0].actual_value.as_deref(), Some("present"));
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
                "check_id": "kvm",
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
fn unknown_check_id_fails_closed() {
    let raw = br#"{
        "schema_version": 1,
        "checks": [
            {
                "check_id": "future_check",
                "check_name": "future",
                "status": "pass"
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
                "check_id": "kvm",
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
                "check_id": "kvm",
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
                "check_id": "kvm",
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
fn diagnostic_maps_host_verifier_failures_to_expected_actual_values() {
    let cases = [
        (
            PreflightError::UnsupportedHostPlatform {
                actual: "Darwin".to_owned(),
            },
            HostPrerequisiteCheckId::OsGate,
            Some("Linux"),
            Some("Darwin"),
        ),
        (
            PreflightError::InvalidCgroupMode {
                actual: "legacy".to_owned(),
            },
            HostPrerequisiteCheckId::CgroupMode,
            Some("unified-v2 or disabled"),
            Some("legacy"),
        ),
        (
            PreflightError::JailIdentityUnavailable {
                field: "jail_uid",
                id: 3000,
            },
            HostPrerequisiteCheckId::JailerIdentity,
            Some("jail_uid present in host identity database"),
            Some("jail_uid id 3000 not found"),
        ),
        (
            PreflightError::KvmCpuExtensionMissing,
            HostPrerequisiteCheckId::KvmCpuExtensions,
            Some("vmx or svm"),
            Some("missing"),
        ),
        (
            PreflightError::NfConntrackCapacityTooLow {
                actual: 128,
                minimum: 1024,
                expected_concurrent_vms: 8,
            },
            HostPrerequisiteCheckId::ConntrackCapacity,
            Some(">= 1024 for 8 concurrent VMs"),
            Some("128"),
        ),
    ];

    for (err, check_id, expected, actual) in cases {
        let check = HostPrerequisiteCheck::from_preflight_error(&err).unwrap();

        assert_eq!(check.check_id, check_id);
        assert_eq!(check.expected_value.as_deref(), expected);
        assert_eq!(check.actual_value.as_deref(), actual);
        assert!(check.remediation.as_ref().unwrap().policy_link.is_some());
    }
}

#[test]
fn diagnostic_maps_missing_firecracker_to_policy_linked_repair() {
    let err = PreflightError::FirecrackerBinaryNotFound {
        path: "/opt/firecracker/bin/firecracker".into(),
    };

    let check = HostPrerequisiteCheck::from_preflight_error(&err).unwrap();

    assert_eq!(check.check_id, HostPrerequisiteCheckId::FirecrackerBinary);
    assert_eq!(check.status, HostPrerequisiteStatus::Fail);
    assert_eq!(
        check.failure_variant,
        Some(HostPrerequisiteFailureKind::FirecrackerBinaryNotFound)
    );
    assert_eq!(
        check.final_path.as_deref(),
        Some(Path::new("/opt/firecracker/bin/firecracker"))
    );
    assert_eq!(check.expected_value.as_deref(), Some("present"));
    assert_eq!(check.actual_value.as_deref(), Some("missing"));
    let remediation = check.remediation.as_ref().unwrap();
    assert_eq!(remediation.id, "install-firecracker-prerequisites");
    assert_eq!(
        remediation.policy_link.as_deref(),
        Some("docs/behaviors/release/host-prerequisite-policy.md")
    );
}

#[test]
fn diagnostic_maps_wrong_jailer_train_to_expected_actual_versions() {
    let err = PreflightError::JailerVersionMismatch {
        expected: "v1.15.1".into(),
        actual: "v1.14.4".into(),
        policy_source: "crates/m80-preflight/src/firecracker_train.rs",
    };

    let check = HostPrerequisiteCheck::from_preflight_error(&err).unwrap();

    assert_eq!(check.check_id, HostPrerequisiteCheckId::JailerBinary);
    assert_eq!(
        check.failure_variant,
        Some(HostPrerequisiteFailureKind::JailerVersionMismatch)
    );
    assert_eq!(check.expected_version.as_deref(), Some("v1.15.1"));
    assert_eq!(check.actual_version.as_deref(), Some("v1.14.4"));
    assert_eq!(
        check.remediation.as_ref().unwrap().policy_link.as_deref(),
        Some("docs/behaviors/release/host-prerequisite-policy.md")
    );
}

#[test]
fn diagnostic_maps_missing_seccomp_filter_to_final_path() {
    let err = PreflightError::FirecrackerSeccompFilterNotFound {
        path: "/opt/firecracker/seccomp/filter.bin".into(),
    };

    let check = HostPrerequisiteCheck::from_preflight_error(&err).unwrap();

    assert_eq!(
        check.check_id,
        HostPrerequisiteCheckId::FirecrackerSeccompFilter
    );
    assert_eq!(
        check.failure_variant,
        Some(HostPrerequisiteFailureKind::FirecrackerSeccompFilterNotFound)
    );
    assert_eq!(
        check.final_path.as_deref(),
        Some(Path::new("/opt/firecracker/seccomp/filter.bin"))
    );
}

#[test]
fn diagnostic_path_fallback_keeps_jailer_identity_under_firecracker_dir() {
    let err = PreflightError::PathIo {
        path: "/opt/firecracker/bin/jailer".into(),
        source: std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied"),
    };

    let check = HostPrerequisiteCheck::from_preflight_error(&err).unwrap();

    assert_eq!(check.check_id, HostPrerequisiteCheckId::JailerBinary);
    assert_eq!(
        check.failure_variant,
        Some(HostPrerequisiteFailureKind::PathIo)
    );
    assert_eq!(
        check.final_path.as_deref(),
        Some(Path::new("/opt/firecracker/bin/jailer"))
    );
    assert_eq!(
        check.expected_value.as_deref(),
        Some("filesystem operation succeeds")
    );
    assert_eq!(check.actual_value.as_deref(), Some("denied"));
}

#[test]
fn diagnostic_maps_bad_kvm_substrate_to_repair_token() {
    let err = PreflightError::KvmNotWritable {
        path: "/dev/kvm".into(),
    };

    let check = HostPrerequisiteCheck::from_preflight_error(&err).unwrap();

    assert_eq!(check.check_id, HostPrerequisiteCheckId::Kvm);
    assert_eq!(
        check.failure_variant,
        Some(HostPrerequisiteFailureKind::KvmNotWritable)
    );
    assert_eq!(check.final_path.as_deref(), Some(Path::new("/dev/kvm")));
    assert_eq!(check.remediation.as_ref().unwrap().id, "repair-kvm");
}

#[test]
fn diagnostic_maps_stale_host_manifest_hash_to_expected_actual_sha() {
    let expected = "a".repeat(64);
    let actual = "b".repeat(64);
    let err = PreflightError::BinaryHashMismatch {
        name: "m80_net_helper",
        path: "/opt/m80/bin/m80-net-helper".into(),
        expected: expected.clone(),
        actual: actual.clone(),
    };

    let check = HostPrerequisiteCheck::from_preflight_error(&err).unwrap();

    assert_eq!(check.check_id, HostPrerequisiteCheckId::NetworkHelper);
    assert_eq!(
        check.failure_variant,
        Some(HostPrerequisiteFailureKind::BinaryHashMismatch)
    );
    assert_eq!(
        check.final_path.as_deref(),
        Some(Path::new("/opt/m80/bin/m80-net-helper"))
    );
    assert_eq!(check.expected_sha256.as_deref(), Some(expected.as_str()));
    assert_eq!(check.actual_sha256.as_deref(), Some(actual.as_str()));
    assert_eq!(
        check.remediation.as_ref().unwrap().policy_link.as_deref(),
        Some("docs/ops/binary-installation.md")
    );
}

#[test]
fn diagnostic_maps_unsupported_bundled_host_asset_policy_failure() {
    let err = PreflightError::HostLaunchMaterialPathMismatch {
        name: "firecracker_seccomp_filter",
        expected: "/opt/firecracker/seccomp/filter.bin".into(),
        actual: "/opt/m80/artifacts/firecracker-seccomp-filter.bin".into(),
    };

    let check = HostPrerequisiteCheck::from_preflight_error(&err).unwrap();

    assert_eq!(
        check.check_id,
        HostPrerequisiteCheckId::FirecrackerSeccompFilter
    );
    assert_eq!(
        check.failure_variant,
        Some(HostPrerequisiteFailureKind::HostLaunchMaterialPathMismatch)
    );
    assert_eq!(
        check.final_path.as_deref(),
        Some(Path::new(
            "/opt/m80/artifacts/firecracker-seccomp-filter.bin"
        ))
    );
    assert_eq!(
        check.remediation.as_ref().unwrap().policy_link.as_deref(),
        Some("docs/ops/binary-installation.md")
    );
}

#[test]
fn hostless_substrate_verifier_emits_result_contract() {
    let discovery =
        verify_host_substrate_fixture(fixture_config(), &HostSubstrateFixture::supported_root())
            .unwrap();
    let check_ids = discovery
        .host_prerequisites
        .checks
        .iter()
        .map(|check| check.check_id)
        .collect::<Vec<_>>();

    assert_eq!(
        discovery.host_prerequisites.schema_version,
        HOST_PREREQUISITE_RESULT_SCHEMA_VERSION
    );
    assert_eq!(
        check_ids,
        [
            HostPrerequisiteCheckId::OsGate,
            HostPrerequisiteCheckId::HostKernelFloor,
            HostPrerequisiteCheckId::Kvm,
            HostPrerequisiteCheckId::CgroupMode,
            HostPrerequisiteCheckId::JailerIdentity,
            HostPrerequisiteCheckId::Privilege,
            HostPrerequisiteCheckId::HostSubstrateProof,
        ]
    );
    assert!(discovery
        .host_prerequisites
        .checks
        .iter()
        .any(
            |check| check.check_id == HostPrerequisiteCheckId::HostSubstrateProof
                && check.status == HostPrerequisiteStatus::Pass
        ));
}
