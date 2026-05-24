//! Each PreflightError variant must expose a non-empty hint via `hint()`.

use caps::Capability;
use m80_image_manifest::{BuildReceiptArtifactKind, InstallProvenanceArtifact, ManifestError};
use m80_preflight::PreflightError;

fn assert_hint(err: &PreflightError) {
    let msg = err.to_string();
    assert!(!msg.is_empty(), "Display for {err:?} must not be empty");
    // Hint is now a separate method, not embedded in Display.
    let hint = err.hint();
    assert!(!hint.is_empty(), "hint() for {err:?} must not be empty");
    // Display must NOT contain embedded hint text (regression guard).
    assert!(
        !msg.contains("hint:"),
        "Display for {err:?} must not embed hint text; got:\n{msg}"
    );
}

#[test]
fn unsupported_host_platform_has_hint() {
    let err = PreflightError::UnsupportedHostPlatform {
        actual: "darwin".into(),
    };
    assert_hint(&err);
    assert!(err.to_string().contains("darwin"));
}

#[test]
fn host_kernel_unsupported_has_hint() {
    let err = PreflightError::HostKernelUnsupported {
        actual: "5.15.0".into(),
        minimum: "6.1".into(),
    };
    assert_hint(&err);
    assert!(err.to_string().contains("5.15.0"));
    assert!(err.to_string().contains("6.1"));
}

#[test]
fn kvm_unavailable_has_hint() {
    let err = PreflightError::KvmUnavailable {
        path: "/dev/kvm".into(),
    };
    assert_hint(&err);
    assert!(err.to_string().contains("/dev/kvm"));
}

#[test]
fn kvm_not_writable_has_hint() {
    let err = PreflightError::KvmNotWritable {
        path: "/dev/kvm".into(),
    };
    assert_hint(&err);
    assert!(err.to_string().contains("/dev/kvm"));
}

#[test]
fn kvm_cpu_extension_missing_has_hint() {
    assert_hint(&PreflightError::KvmCpuExtensionMissing);
}

#[test]
fn invalid_cgroup_mode_has_hint() {
    let err = PreflightError::InvalidCgroupMode {
        actual: "legacy".into(),
    };
    assert_hint(&err);
    assert!(err.to_string().contains("legacy"));
}

#[test]
fn invalid_jail_identity_has_hint() {
    let err = PreflightError::InvalidJailIdentity {
        field: "jail_uid",
        value: "not-a-uid".into(),
    };
    assert_hint(&err);
    assert!(err.to_string().contains("jail_uid"));
    assert!(err.to_string().contains("not-a-uid"));
}

#[test]
fn jail_identity_unavailable_has_hint() {
    let err = PreflightError::JailIdentityUnavailable {
        field: "jail_gid",
        id: 3000,
    };
    assert_hint(&err);
    assert!(err.to_string().contains("jail_gid"));
    assert!(err.to_string().contains("3000"));
}

#[test]
fn cpu_vulnerability_detected_has_hint() {
    let err = PreflightError::CpuVulnerabilityDetected {
        id: "mds".into(),
        detail: "Vulnerable: no microcode".into(),
    };
    assert_hint(&err);
    assert!(err.to_string().contains("mds"));
    assert!(err.to_string().contains("Vulnerable"));
}

#[test]
fn cgroup_v2_unavailable_has_hint() {
    assert_hint(&PreflightError::CgroupV2Unavailable);
}

#[test]
fn vsock_unavailable_has_hint() {
    assert_hint(&PreflightError::VsockUnavailable);
}

#[test]
fn tun_unavailable_has_hint() {
    assert_hint(&PreflightError::TunUnavailable);
}

#[test]
fn nf_conntrack_unavailable_has_hint() {
    assert_hint(&PreflightError::NfConntrackUnavailable);
}

#[test]
fn bridge_netfilter_unavailable_has_hint() {
    assert_hint(&PreflightError::BridgeNetfilterUnavailable);
}

#[test]
fn bridge_nf_call_iptables_disabled_has_hint() {
    assert_hint(&PreflightError::BridgeNfCallIptablesDisabled {
        actual: "0".to_owned(),
    });
}

#[test]
fn nf_conntrack_capacity_too_low_has_hint() {
    assert_hint(&PreflightError::NfConntrackCapacityTooLow {
        actual: 10_000,
        minimum: 16_000,
        expected_concurrent_vms: 8,
    });
}

#[test]
fn invalid_nf_conntrack_max_has_hint() {
    assert_hint(&PreflightError::InvalidNfConntrackMax {
        actual: "invalid".to_owned(),
    });
}

#[test]
fn invalid_expected_concurrent_vms_has_hint() {
    assert_hint(&PreflightError::InvalidExpectedConcurrentVms {
        actual: "0".to_owned(),
    });
}

#[test]
fn kernel_modules_missing_has_hint() {
    assert_hint(&PreflightError::KernelModulesMissing {
        missing: vec!["tap".into()],
    });
}

#[test]
fn ksm_enabled_has_hint() {
    let err = PreflightError::KsmEnabled {
        actual: "1".to_owned(),
    };
    assert_hint(&err);
    assert!(err.hint().contains("M80_SKIP_CHECK_KSM=1"));
}

#[test]
fn smt_enabled_has_hint() {
    let err = PreflightError::SmtEnabled {
        actual: "on".to_owned(),
    };
    assert_hint(&err);
    assert!(err.hint().contains("M80_SMT_CHECK"));
}

#[test]
fn swap_active_has_hint() {
    let err = PreflightError::SwapActive {
        devices: vec!["/swapfile".to_owned()],
    };
    assert_hint(&err);
    assert!(err.hint().contains("swapoff"));
}

#[test]
fn nested_virt_enabled_has_hint() {
    let err = PreflightError::NestedVirtEnabled {
        vendor: "intel".to_owned(),
    };
    assert_hint(&err);
    assert!(err.hint().contains("M80_SKIP_CHECK_NESTED_VIRT=1"));
}

#[test]
fn kvm_timer_floor_unset_has_hint() {
    let err = PreflightError::KvmTimerFloorUnset {
        actual: "0".to_owned(),
    };
    assert_hint(&err);
    assert!(err.hint().contains("M80_SKIP_CHECK_KVM_TIMER=1"));
}

#[test]
fn privilege_unavailable_has_hint() {
    assert_hint(&PreflightError::PrivilegeUnavailable {
        missing_caps: vec![Capability::CAP_NET_ADMIN],
    });
}

#[test]
fn firecracker_binary_not_found_has_hint() {
    assert_hint(&PreflightError::FirecrackerBinaryNotFound {
        path: "/opt/firecracker/bin/firecracker".into(),
    });
}

#[test]
fn firecracker_version_mismatch_has_hint() {
    assert_hint(&PreflightError::FirecrackerVersionMismatch {
        expected: "v1.15.1".into(),
        actual: "v1.14.0".into(),
        policy_source: "crates/m80-preflight/src/firecracker_train.rs",
    });
}

#[test]
fn firecracker_version_output_malformed_has_hint() {
    assert_hint(&PreflightError::FirecrackerVersionOutputMalformed {
        actual: "not-firecracker".into(),
        policy_source: "crates/m80-preflight/src/firecracker_train.rs",
    });
}

#[test]
fn firecracker_cve_floor_violation_has_hint() {
    assert_hint(&PreflightError::FirecrackerCveFloorViolation {
        cve_id: "CVE-2026-5747".into(),
        actual: "v1.15.0".into(),
        expected: "v1.14.4 or v1.15.1".into(),
        policy_source: "crates/m80-preflight/src/cve_floor.rs",
    });
}

#[test]
fn firecracker_seccomp_filter_not_found_has_hint() {
    assert_hint(&PreflightError::FirecrackerSeccompFilterNotFound {
        path: "/opt/firecracker/bin/firecracker-seccomp-filter.bin".into(),
    });
}

#[test]
fn firecracker_seccomp_filter_empty_has_hint() {
    assert_hint(&PreflightError::FirecrackerSeccompFilterEmpty {
        path: "/opt/firecracker/bin/firecracker-seccomp-filter.bin".into(),
    });
}

#[test]
fn jailer_binary_not_found_has_hint() {
    assert_hint(&PreflightError::JailerBinaryNotFound {
        path: "/opt/firecracker/bin/jailer".into(),
    });
}

#[test]
fn jailer_version_command_failed_has_hint() {
    assert_hint(&PreflightError::JailerVersionCommandFailed {
        path: "/opt/firecracker/bin/jailer".into(),
        status: "exit status: 1".into(),
    });
}

#[test]
fn jailer_version_output_malformed_has_hint() {
    assert_hint(&PreflightError::JailerVersionOutputMalformed {
        actual: "not-jailer".into(),
        policy_source: "crates/m80-preflight/src/firecracker_train.rs",
    });
}

#[test]
fn jailer_version_mismatch_has_hint() {
    assert_hint(&PreflightError::JailerVersionMismatch {
        expected: "v1.15.1".into(),
        actual: "v1.14.4".into(),
        policy_source: "crates/m80-preflight/src/firecracker_train.rs",
    });
}

#[test]
fn jailer_harden_binary_not_found_has_hint() {
    assert_hint(&PreflightError::JailerHardenBinaryNotFound {
        path: "/opt/m80/bin/m80-jailer-harden".into(),
    });
}

#[test]
fn net_helper_binary_not_found_has_hint() {
    assert_hint(&PreflightError::NetHelperBinaryNotFound {
        path: "/opt/m80/bin/m80-net-helper".into(),
    });
}

#[test]
fn host_binary_manifest_has_hint() {
    let inner = ManifestError::UnsupportedHostBinariesSchemaVersion(99);
    assert_hint(&PreflightError::HostBinaryManifest(inner));
}

#[test]
fn host_binary_missing_has_hint() {
    assert_hint(&PreflightError::HostBinaryMissing {
        name: "firecracker",
    });
}

#[test]
fn host_binary_duplicate_has_hint() {
    assert_hint(&PreflightError::HostBinaryDuplicate { name: "jailer" });
}

#[test]
fn host_binary_path_mismatch_has_hint() {
    assert_hint(&PreflightError::HostBinaryPathMismatch {
        name: "firecracker",
        expected: "/opt/firecracker/bin/firecracker".into(),
        actual: "/tmp/firecracker".into(),
    });
}

#[test]
fn binary_hash_mismatch_has_hint() {
    assert_hint(&PreflightError::BinaryHashMismatch {
        name: "m80",
        path: "/opt/m80/bin/m80".into(),
        expected: "a".repeat(64),
        actual: "b".repeat(64),
    });
}

#[test]
fn host_binary_permission_has_hint() {
    assert_hint(&PreflightError::HostBinaryPermission {
        name: "m80_net_helper",
        path: "/opt/m80/bin/m80-net-helper".into(),
        reason: "owner is not root:root",
    });
}

#[test]
fn host_binary_version_command_failed_has_hint() {
    assert_hint(&PreflightError::HostBinaryVersionCommandFailed {
        name: "m80_net_helper",
        path: "/opt/m80/bin/m80-net-helper".into(),
        status: "exit status: 1".into(),
    });
}

#[test]
fn host_binary_version_mismatch_has_hint() {
    assert_hint(&PreflightError::HostBinaryVersionMismatch {
        name: "firecracker",
        path: "/opt/firecracker/bin/firecracker".into(),
        expected: "v1.15.1".into(),
        actual: "v1.15.2".into(),
    });
}

#[test]
fn host_launch_material_missing_has_hint() {
    assert_hint(&PreflightError::HostLaunchMaterialMissing {
        name: "firecracker_seccomp_filter",
    });
}

#[test]
fn host_launch_material_duplicate_has_hint() {
    assert_hint(&PreflightError::HostLaunchMaterialDuplicate {
        name: "firecracker_seccomp_filter",
    });
}

#[test]
fn host_launch_material_path_mismatch_has_hint() {
    assert_hint(&PreflightError::HostLaunchMaterialPathMismatch {
        name: "firecracker_seccomp_filter",
        expected: "/opt/firecracker/bin/firecracker-seccomp-filter.bin".into(),
        actual: "/tmp/firecracker-seccomp-filter.bin".into(),
    });
}

#[test]
fn host_launch_material_hash_mismatch_has_hint() {
    assert_hint(&PreflightError::HostLaunchMaterialHashMismatch {
        name: "firecracker_seccomp_filter",
        path: "/opt/firecracker/bin/firecracker-seccomp-filter.bin".into(),
        expected: "a".repeat(64),
        actual: "b".repeat(64),
    });
}

#[test]
fn host_launch_material_permission_has_hint() {
    assert_hint(&PreflightError::HostLaunchMaterialPermission {
        name: "firecracker_seccomp_filter",
        path: "/opt/firecracker/bin/firecracker-seccomp-filter.bin".into(),
        reason: "owner is not root:root",
    });
}

#[test]
fn host_launch_material_version_mismatch_has_hint() {
    assert_hint(&PreflightError::HostLaunchMaterialVersionMismatch {
        name: "firecracker_seccomp_filter",
        path: "/opt/firecracker/bin/firecracker-seccomp-filter.bin".into(),
        expected: "v1.15.1".into(),
        actual: "v1.15.2".into(),
    });
}

#[test]
fn non_absolute_path_has_hint() {
    assert_hint(&PreflightError::NonAbsolutePath {
        kind: "rootfs".into(),
        path: "rootfs.ext4".into(),
    });
}

#[test]
fn kernel_not_found_has_hint() {
    assert_hint(&PreflightError::KernelNotFound);
}

#[test]
fn rootfs_not_found_has_hint() {
    assert_hint(&PreflightError::RootfsNotFound);
}

#[test]
fn artifact_directory_writable_has_hint() {
    assert_hint(&PreflightError::ArtifactDirectoryWritable {
        path: "/opt/m80/artifacts".into(),
        mode: 0o777,
    });
}

#[test]
fn artifact_file_writable_has_hint() {
    assert_hint(&PreflightError::ArtifactFileWritable {
        path: "/opt/m80/artifacts/rootfs.ext4".into(),
        mode: 0o666,
    });
}

#[test]
fn manifest_error_has_hint() {
    let inner = ManifestError::UnsupportedSchemaVersion(99);
    assert_hint(&PreflightError::Manifest(inner));
}

#[test]
fn build_receipt_error_has_hint() {
    let inner = ManifestError::UnsupportedBuildReceiptSchemaVersion(99);
    assert_hint(&PreflightError::BuildReceipt(inner));
}

#[test]
fn install_provenance_errors_have_hints() {
    let inner = ManifestError::UnsupportedInstallProvenanceSchemaVersion(99);
    assert_hint(&PreflightError::InstallProvenance(inner));
    assert_hint(&PreflightError::InstallProvenanceMissing {
        path: "/opt/m80/artifacts/install-provenance.json".into(),
    });
    assert_hint(&PreflightError::InstallProvenanceTransformMissing {
        artifact: InstallProvenanceArtifact::GuestManifest,
    });
    assert_hint(&PreflightError::InstallProvenanceTransformDuplicate {
        artifact: InstallProvenanceArtifact::GuestManifest,
    });
    assert_hint(&PreflightError::InstallProvenancePathMismatch {
        artifact: InstallProvenanceArtifact::GuestManifest,
        expected: "/opt/m80/artifacts/output.ext4.manifest.json".into(),
        actual: "/tmp/output.ext4.manifest.json".into(),
    });
    assert_hint(&PreflightError::InstallProvenanceHashMismatch {
        path: "/opt/m80/artifacts/output.ext4.manifest.json".into(),
        expected: "a".repeat(64),
        actual: "b".repeat(64),
    });
}

#[test]
fn build_receipt_path_mismatch_has_hint() {
    assert_hint(&PreflightError::BuildReceiptPathMismatch {
        expected: "/opt/m80/artifacts/output.ext4.manifest.json".into(),
        actual: "/tmp/output.ext4.manifest.json".into(),
    });
}

#[test]
fn build_receipt_manifest_mismatch_has_hint() {
    assert_hint(&PreflightError::BuildReceiptManifestMismatch {
        path: "/opt/m80/artifacts/output.ext4.manifest.json".into(),
        expected: "a".repeat(64),
        actual: "b".repeat(64),
    });
}

#[test]
fn build_receipt_artifact_errors_have_hints() {
    assert_hint(&PreflightError::BuildReceiptArtifactMissing {
        kind: BuildReceiptArtifactKind::KernelImage,
    });
    assert_hint(&PreflightError::BuildReceiptArtifactDuplicate {
        kind: BuildReceiptArtifactKind::KernelImage,
    });
    assert_hint(&PreflightError::BuildReceiptArtifactPathMismatch {
        kind: BuildReceiptArtifactKind::KernelImage,
        expected: "/opt/m80/artifacts/vmlinux".into(),
        actual: "/tmp/vmlinux".into(),
    });
    assert_hint(&PreflightError::BuildReceiptArtifactHashMismatch {
        kind: BuildReceiptArtifactKind::KernelImage,
        expected: "a".repeat(64),
        actual: "b".repeat(64),
    });
}

#[test]
fn run_root_unavailable_has_hint() {
    assert_hint(&PreflightError::RunRootUnavailable {
        reason: "directory does not exist: /var/run/m80".into(),
    });
}

#[test]
fn storage_helper_missing_has_hint() {
    assert_hint(&PreflightError::StorageHelperMissing("mkfs.ext4".into()));
}

#[test]
fn io_error_has_hint() {
    let io_err = std::io::Error::other("disk full");
    assert_hint(&PreflightError::PathIo {
        path: std::path::PathBuf::from("/tmp/artifact"),
        source: io_err,
    });
}

#[test]
fn capability_read_has_hint() {
    use caps::errors::CapsError;
    let err = PreflightError::CapabilityRead(CapsError::from("kernel returned EINVAL"));
    assert_hint(&err);
    assert!(err.to_string().contains("capability read failed"));
}
