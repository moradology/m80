//! Versioned host-prerequisite proof result.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::{CheckRow, Discovery, PreflightError};

mod check_id;
mod failure;

pub use check_id::HostPrerequisiteCheckId;
pub use failure::HostPrerequisiteFailureKind;

/// Current schema version for [`HostPrerequisiteResult`].
pub const HOST_PREREQUISITE_RESULT_SCHEMA_VERSION: u32 = 1;

const HOST_SETUP_DOC: &str = "docs/ops/host-setup.md";
const BINARY_INSTALLATION_DOC: &str = "docs/ops/binary-installation.md";
const HOST_PREREQUISITE_POLICY_DOC: &str = "docs/behaviors/release/host-prerequisite-policy.md";
const FIRECRACKER_CVE_FLOOR_DOC: &str = "docs/security/firecracker-cve-floor.md";

/// Operator must install or repair the official Firecracker, jailer, or
/// Firecracker seccomp filter prerequisites named by the policy doc.
pub const REPAIR_INSTALL_FIRECRACKER_PREREQUISITES: &str = "install-firecracker-prerequisites";
/// Operator must make `/dev/kvm` present and writable for the run identity.
pub const REPAIR_KVM: &str = "repair-kvm";
/// Operator must run on the documented cgroup mode or disable cgroup checks.
pub const REPAIR_CGROUP_MODE: &str = "repair-cgroup-mode";
/// Operator must run m80 with the documented root/capability posture.
pub const REPAIR_PRIVILEGE: &str = "repair-privilege";
/// Operator must upgrade the official Firecracker train past the active CVE floor.
pub const REPAIR_UPGRADE_FIRECRACKER_CVE_FLOOR: &str = "upgrade-firecracker-cve-floor";
/// Reinstall m80-owned helpers or regenerate the installed host-binaries manifest.
pub const REPAIR_HOST_BINARIES_MANIFEST: &str = "repair-host-binaries-manifest";
/// Generic host posture fallback for substrate failures outside a tighter token.
pub const REPAIR_HOST_SETUP: &str = "repair-host-setup";
/// Reinstall the pinned m80 release that owns a broken m80 helper.
pub const REPAIR_REINSTALL_M80_RELEASE: &str = "reinstall-m80-release";

/// Machine-readable host-prerequisite proof shared by install, preflight,
/// diagnostics, and release proof artifacts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostPrerequisiteResult {
    /// Schema version for fail-closed readers.
    pub schema_version: u32,
    /// Ordered prerequisite checks.
    pub checks: Vec<HostPrerequisiteCheck>,
}

impl HostPrerequisiteResult {
    /// Build a schema-current result from already structured checks.
    #[must_use]
    pub fn new(checks: Vec<HostPrerequisiteCheck>) -> Self {
        Self {
            schema_version: HOST_PREREQUISITE_RESULT_SCHEMA_VERSION,
            checks,
        }
    }

    /// Build a schema-current success result from preflight table rows.
    pub fn from_success_rows(rows: &[CheckRow]) -> Result<Self, HostPrerequisiteResultError> {
        Ok(Self::new(
            rows.iter()
                .map(HostPrerequisiteCheck::from_success_row)
                .collect::<Result<Vec<_>, _>>()?,
        ))
    }

    /// Build a schema-current success result from a complete full-preflight
    /// report and fail if stable check ids drift out of registry order.
    pub fn from_full_report_rows(rows: &[CheckRow]) -> Result<Self, HostPrerequisiteResultError> {
        let result = Self::from_success_rows(rows)?;
        result.validate_full_report_check_id_order()?;
        Ok(result)
    }

    /// Build a schema-current result from a successful full preflight
    /// discovery, preserving structured fields that table rows only render as
    /// human text.
    pub fn from_discovery(discovery: &Discovery) -> Result<Self, HostPrerequisiteResultError> {
        let mut result = Self::from_success_rows(&discovery.report)?;
        for check in &mut result.checks {
            match check.check_id {
                HostPrerequisiteCheckId::FirecrackerBinary => {
                    check.final_path = Some(discovery.firecracker_bin.clone());
                    check.expected_version =
                        Some(discovery.manifest.expected_firecracker_version.clone());
                    check.actual_version = Some(discovery.firecracker_version.clone());
                }
                HostPrerequisiteCheckId::JailerBinary => {
                    check.final_path = Some(discovery.jailer_bin.clone());
                    check.expected_version =
                        Some(discovery.manifest.expected_firecracker_version.clone());
                    check.actual_version = Some(discovery.jailer_version.clone());
                }
                HostPrerequisiteCheckId::FirecrackerSeccompFilter => {
                    check.final_path = Some(discovery.firecracker_seccomp_filter.clone());
                }
                HostPrerequisiteCheckId::JailerHardeningWrapper => {
                    check.final_path = Some(discovery.jailer_harden_bin.clone());
                }
                HostPrerequisiteCheckId::NetworkHelper => {
                    check.final_path = Some(discovery.net_helper_bin.clone());
                }
                HostPrerequisiteCheckId::KernelImage => {
                    check.final_path = Some(discovery.kernel.clone());
                }
                HostPrerequisiteCheckId::RootfsManifest => {
                    check.final_path = Some(discovery.rootfs.clone());
                }
                HostPrerequisiteCheckId::RunRoot => {
                    check.final_path = Some(discovery.run_root.clone());
                }
                _ => {}
            }
        }
        Ok(result)
    }

    /// Build a schema-current result from a successful full preflight
    /// discovery and validate the complete check-id order contract.
    pub fn from_full_discovery(discovery: &Discovery) -> Result<Self, HostPrerequisiteResultError> {
        let result = Self::from_discovery(discovery)?;
        result.validate_full_report_check_id_order()?;
        Ok(result)
    }

    /// Decode and validate a JSON proof result.
    pub fn from_json_slice(bytes: &[u8]) -> Result<Self, HostPrerequisiteResultError> {
        let result: Self =
            serde_json::from_slice(bytes).map_err(HostPrerequisiteResultError::Json)?;
        result.validate()?;
        Ok(result)
    }

    /// Build a one-check failed result for a preflight verifier error.
    #[must_use]
    pub fn from_preflight_error(error: &PreflightError) -> Option<Self> {
        HostPrerequisiteCheck::from_preflight_error(error).map(|check| Self::new(vec![check]))
    }

    /// Validate schema version and failure remediation invariants.
    pub fn validate(&self) -> Result<(), HostPrerequisiteResultError> {
        if self.schema_version != HOST_PREREQUISITE_RESULT_SCHEMA_VERSION {
            return Err(HostPrerequisiteResultError::UnsupportedSchemaVersion {
                expected: HOST_PREREQUISITE_RESULT_SCHEMA_VERSION,
                actual: self.schema_version,
            });
        }

        for check in &self.checks {
            if check.status != HostPrerequisiteStatus::Fail {
                continue;
            }
            if check.failure_variant.is_none() {
                return Err(HostPrerequisiteResultError::MissingFailureVariant {
                    check_name: check.check_name.clone(),
                });
            }
            let Some(remediation) = &check.remediation else {
                return Err(HostPrerequisiteResultError::MissingRemediationToken {
                    check_name: check.check_name.clone(),
                });
            };
            if remediation.id.trim().is_empty() {
                return Err(HostPrerequisiteResultError::MissingRemediationToken {
                    check_name: check.check_name.clone(),
                });
            }
            let has_command = remediation
                .command
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty());
            let has_policy_link = remediation
                .policy_link
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty());
            if !has_command && !has_policy_link {
                return Err(HostPrerequisiteResultError::MissingRemediationTarget {
                    check_name: check.check_name.clone(),
                });
            }
        }

        Ok(())
    }

    fn validate_full_report_check_id_order(&self) -> Result<(), HostPrerequisiteResultError> {
        for (duplicate_index, check) in self.checks.iter().enumerate() {
            if let Some(first_index) = self.checks[..duplicate_index]
                .iter()
                .position(|seen| seen.check_id == check.check_id)
            {
                return Err(HostPrerequisiteResultError::DuplicateCheckId {
                    check_id: check.check_id.as_str(),
                    first_index,
                    duplicate_index,
                });
            }
        }

        let actual = self
            .checks
            .iter()
            .map(|check| check.check_id)
            .collect::<Vec<_>>();
        for (expected_index, expected) in HostPrerequisiteCheckId::ALL.iter().copied().enumerate() {
            match actual.get(expected_index).copied() {
                Some(actual) if actual == expected => {}
                Some(actual) => {
                    if !self.checks.iter().any(|check| check.check_id == expected) {
                        return Err(HostPrerequisiteResultError::MissingCheckId {
                            check_id: expected.as_str(),
                            expected_index,
                        });
                    }
                    return Err(HostPrerequisiteResultError::OutOfOrderCheckId {
                        expected_check_id: expected.as_str(),
                        actual_check_id: actual.as_str(),
                        index: expected_index,
                    });
                }
                None => {
                    return Err(HostPrerequisiteResultError::MissingCheckId {
                        check_id: expected.as_str(),
                        expected_index,
                    });
                }
            }
        }

        Ok(())
    }
}

/// One host-prerequisite check in a [`HostPrerequisiteResult`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostPrerequisiteCheck {
    /// Stable machine-readable check identity.
    pub check_id: HostPrerequisiteCheckId,
    /// Stable human-readable check name.
    pub check_name: String,
    /// Final host path observed or verified by the check.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub final_path: Option<PathBuf>,
    /// Expected non-version scalar value, when the check has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_value: Option<String>,
    /// Actual observed non-version scalar value, when the check has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actual_value: Option<String>,
    /// Expected version, when the check has a version source of truth.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_version: Option<String>,
    /// Actual observed version, when the check probes one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actual_version: Option<String>,
    /// Expected sha256, when the check has installed-byte identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_sha256: Option<String>,
    /// Actual observed sha256, when the check hashes a file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actual_sha256: Option<String>,
    /// Expected Unix mode bits, when the check constrains mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_mode: Option<u32>,
    /// Actual Unix mode bits observed on disk.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actual_mode: Option<u32>,
    /// Expected owner, when the check constrains ownership.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_owner: Option<HostPrerequisiteOwner>,
    /// Actual owner observed on disk.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actual_owner: Option<HostPrerequisiteOwner>,
    /// Pass/fail status.
    pub status: HostPrerequisiteStatus,
    /// Typed failure variant for failed checks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_variant: Option<HostPrerequisiteFailureKind>,
    /// Stable remediation token and command or policy link for failed checks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remediation: Option<HostPrerequisiteRemediation>,
}

impl HostPrerequisiteCheck {
    /// Build a passing check.
    #[must_use]
    pub fn pass(check_id: HostPrerequisiteCheckId) -> Self {
        Self {
            check_id,
            check_name: check_id.check_name().to_string(),
            final_path: None,
            expected_value: None,
            actual_value: None,
            expected_version: None,
            actual_version: None,
            expected_sha256: None,
            actual_sha256: None,
            expected_mode: None,
            actual_mode: None,
            expected_owner: None,
            actual_owner: None,
            status: HostPrerequisiteStatus::Pass,
            failure_variant: None,
            remediation: None,
        }
    }

    /// Build a failing check.
    #[must_use]
    pub fn fail(
        check_id: HostPrerequisiteCheckId,
        failure_variant: HostPrerequisiteFailureKind,
        remediation: HostPrerequisiteRemediation,
    ) -> Self {
        Self {
            status: HostPrerequisiteStatus::Fail,
            failure_variant: Some(failure_variant),
            remediation: Some(remediation),
            ..Self::pass(check_id)
        }
    }

    /// Project a typed preflight error into the host-prerequisite diagnostic
    /// schema used by JSON preflight output, release proofs, and text repair
    /// rendering.
    #[must_use]
    pub fn from_preflight_error(error: &PreflightError) -> Option<Self> {
        let failure_variant = HostPrerequisiteFailureKind::from_preflight_error(error)?;
        let mut check = Self::fail(
            check_id_for_preflight_error(error),
            failure_variant,
            remediation_for_preflight_error(error),
        );
        apply_preflight_error_fields(&mut check, error);
        Some(check)
    }

    /// Override the human label while preserving the stable machine identity.
    #[must_use]
    pub fn with_check_name(mut self, check_name: impl Into<String>) -> Self {
        self.check_name = check_name.into();
        self
    }

    /// Attach a final host path.
    #[must_use]
    pub fn with_final_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.final_path = Some(path.into());
        self
    }

    /// Attach expected and actual non-version scalar facts.
    #[must_use]
    pub fn with_values(mut self, expected: impl Into<String>, actual: impl Into<String>) -> Self {
        self.expected_value = Some(expected.into());
        self.actual_value = Some(actual.into());
        self
    }

    /// Attach expected and actual version facts.
    #[must_use]
    pub fn with_versions(mut self, expected: impl Into<String>, actual: impl Into<String>) -> Self {
        self.expected_version = Some(expected.into());
        self.actual_version = Some(actual.into());
        self
    }

    /// Attach expected and actual sha256 facts.
    #[must_use]
    pub fn with_sha256(mut self, expected: impl Into<String>, actual: impl Into<String>) -> Self {
        self.expected_sha256 = Some(expected.into());
        self.actual_sha256 = Some(actual.into());
        self
    }

    /// Attach expected and actual mode facts.
    #[must_use]
    pub fn with_modes(mut self, expected: u32, actual: u32) -> Self {
        self.expected_mode = Some(expected);
        self.actual_mode = Some(actual);
        self
    }

    /// Attach expected and actual owner facts.
    #[must_use]
    pub fn with_owners(
        mut self,
        expected: HostPrerequisiteOwner,
        actual: HostPrerequisiteOwner,
    ) -> Self {
        self.expected_owner = Some(expected);
        self.actual_owner = Some(actual);
        self
    }

    fn from_success_row(row: &CheckRow) -> Result<Self, HostPrerequisiteResultError> {
        if !row.passed {
            return Err(HostPrerequisiteResultError::UnexpectedFailedSuccessRow {
                check_name: row.label.clone(),
            });
        }
        Ok(Self::pass(row.check_id).with_check_name(row.label.clone()))
    }
}

fn check_id_for_preflight_error(error: &PreflightError) -> HostPrerequisiteCheckId {
    match error {
        PreflightError::UnsupportedHostPlatform { .. } => HostPrerequisiteCheckId::OsGate,
        PreflightError::HostKernelUnsupported { .. } => HostPrerequisiteCheckId::HostKernelFloor,
        PreflightError::KvmUnavailable { .. } | PreflightError::KvmNotWritable { .. } => {
            HostPrerequisiteCheckId::Kvm
        }
        PreflightError::KvmCpuExtensionMissing => HostPrerequisiteCheckId::KvmCpuExtensions,
        PreflightError::InvalidCgroupMode { .. } | PreflightError::CgroupV2Unavailable => {
            HostPrerequisiteCheckId::CgroupMode
        }
        PreflightError::InvalidJailIdentity { .. }
        | PreflightError::JailIdentityUnavailable { .. } => HostPrerequisiteCheckId::JailerIdentity,
        PreflightError::CpuVulnerabilityDetected { .. } => {
            HostPrerequisiteCheckId::CpuVulnerabilities
        }
        PreflightError::VsockUnavailable
        | PreflightError::TunUnavailable
        | PreflightError::NfConntrackUnavailable
        | PreflightError::BridgeNetfilterUnavailable
        | PreflightError::BridgeNfCallIptablesDisabled { .. }
        | PreflightError::KernelModulesMissing { .. } => HostPrerequisiteCheckId::KernelModules,
        PreflightError::KsmEnabled { .. } => HostPrerequisiteCheckId::KsmDisabled,
        PreflightError::SmtEnabled { .. } => HostPrerequisiteCheckId::SmtDisabled,
        PreflightError::SwapActive { .. } => HostPrerequisiteCheckId::SwapDisabled,
        PreflightError::NestedVirtEnabled { .. } => HostPrerequisiteCheckId::NestedVirtDisabled,
        PreflightError::NfConntrackCapacityTooLow { .. }
        | PreflightError::InvalidNfConntrackMax { .. }
        | PreflightError::InvalidExpectedConcurrentVms { .. } => {
            HostPrerequisiteCheckId::ConntrackCapacity
        }
        PreflightError::PrivilegeUnavailable { .. } | PreflightError::CapabilityRead(_) => {
            HostPrerequisiteCheckId::Privilege
        }
        PreflightError::FirecrackerBinaryNotFound { .. }
        | PreflightError::FirecrackerVersionMismatch { .. }
        | PreflightError::FirecrackerVersionCommandFailed { .. }
        | PreflightError::FirecrackerVersionOutputMalformed { .. }
        | PreflightError::FirecrackerCveFloorViolation { .. } => {
            HostPrerequisiteCheckId::FirecrackerBinary
        }
        PreflightError::FirecrackerSeccompFilterNotFound { .. }
        | PreflightError::FirecrackerSeccompFilterEmpty { .. } => {
            HostPrerequisiteCheckId::FirecrackerSeccompFilter
        }
        PreflightError::JailerBinaryNotFound { .. }
        | PreflightError::JailerVersionCommandFailed { .. }
        | PreflightError::JailerVersionOutputMalformed { .. }
        | PreflightError::JailerVersionMismatch { .. } => HostPrerequisiteCheckId::JailerBinary,
        PreflightError::JailerHardenBinaryNotFound { .. } => {
            HostPrerequisiteCheckId::JailerHardeningWrapper
        }
        PreflightError::NetHelperBinaryNotFound { .. } => HostPrerequisiteCheckId::NetworkHelper,
        PreflightError::HostBinaryManifest(_)
        | PreflightError::HostBinaryMissing { .. }
        | PreflightError::HostBinaryDuplicate { .. } => HostPrerequisiteCheckId::HostBinaryManifest,
        PreflightError::HostBinaryPathMismatch { name, .. }
        | PreflightError::BinaryHashMismatch { name, .. }
        | PreflightError::HostBinaryPermission { name, .. }
        | PreflightError::HostBinaryVersionCommandFailed { name, .. }
        | PreflightError::HostBinaryVersionMismatch { name, .. } => {
            check_id_for_host_binary_name(name)
        }
        PreflightError::HostLaunchMaterialMissing { .. }
        | PreflightError::HostLaunchMaterialDuplicate { .. }
        | PreflightError::HostLaunchMaterialPathMismatch { .. }
        | PreflightError::HostLaunchMaterialHashMismatch { .. }
        | PreflightError::HostLaunchMaterialPermission { .. }
        | PreflightError::HostLaunchMaterialVersionMismatch { .. } => {
            HostPrerequisiteCheckId::FirecrackerSeccompFilter
        }
        PreflightError::NonAbsolutePath { kind, .. } => check_id_for_non_absolute_kind(kind),
        PreflightError::PathIo { path, .. } => check_id_for_path(path),
        PreflightError::SystemIo { operation, .. } => check_id_for_system_operation(operation),
        _ => HostPrerequisiteCheckId::RootfsManifest,
    }
}

fn check_id_for_host_binary_name(name: &str) -> HostPrerequisiteCheckId {
    match name {
        "firecracker" => HostPrerequisiteCheckId::FirecrackerBinary,
        "jailer" => HostPrerequisiteCheckId::JailerBinary,
        "m80_jailer_harden" => HostPrerequisiteCheckId::JailerHardeningWrapper,
        "m80_net_helper" => HostPrerequisiteCheckId::NetworkHelper,
        _ => HostPrerequisiteCheckId::HostBinaryManifest,
    }
}

fn check_id_for_non_absolute_kind(kind: &str) -> HostPrerequisiteCheckId {
    if kind.contains("firecracker_seccomp_filter") || kind.contains("launch material") {
        HostPrerequisiteCheckId::FirecrackerSeccompFilter
    } else if kind.contains("firecracker") {
        HostPrerequisiteCheckId::FirecrackerBinary
    } else if kind.contains("jailer") {
        HostPrerequisiteCheckId::JailerBinary
    } else if kind.contains("net_helper") {
        HostPrerequisiteCheckId::NetworkHelper
    } else if kind.contains("kernel") {
        HostPrerequisiteCheckId::KernelImage
    } else if kind.contains("rootfs") {
        HostPrerequisiteCheckId::RootfsManifest
    } else {
        HostPrerequisiteCheckId::HostBinaryManifest
    }
}

fn check_id_for_path(path: &std::path::Path) -> HostPrerequisiteCheckId {
    match path.to_str() {
        Some("/dev/kvm") => HostPrerequisiteCheckId::Kvm,
        Some("/proc/cpuinfo") => HostPrerequisiteCheckId::KvmCpuExtensions,
        Some("/sys/kernel/mm/ksm/run") => HostPrerequisiteCheckId::KsmDisabled,
        Some("/sys/devices/system/cpu/smt/control") => HostPrerequisiteCheckId::SmtDisabled,
        Some("/proc/swaps") => HostPrerequisiteCheckId::SwapDisabled,
        Some("/sys/module/kvm_intel/parameters/nested")
        | Some("/sys/module/kvm_amd/parameters/nested") => {
            HostPrerequisiteCheckId::NestedVirtDisabled
        }
        Some("/proc/modules") | Some("/proc/sys/net/bridge/bridge-nf-call-iptables") => {
            HostPrerequisiteCheckId::KernelModules
        }
        Some("/proc/sys/net/netfilter/nf_conntrack_max") => {
            HostPrerequisiteCheckId::ConntrackCapacity
        }
        Some(value) if value.contains("seccomp") => {
            HostPrerequisiteCheckId::FirecrackerSeccompFilter
        }
        Some(value) if value.contains("jailer") => HostPrerequisiteCheckId::JailerBinary,
        Some(value) if value.contains("net-helper") || value.contains("net_helper") => {
            HostPrerequisiteCheckId::NetworkHelper
        }
        Some(value) if value.contains("firecracker") => HostPrerequisiteCheckId::FirecrackerBinary,
        Some(value) if value.contains("vmlinux") || value.contains("kernel") => {
            HostPrerequisiteCheckId::KernelImage
        }
        Some(value) if value.contains("rootfs") || value.ends_with(".ext4") => {
            HostPrerequisiteCheckId::RootfsManifest
        }
        _ => HostPrerequisiteCheckId::HostBinaryManifest,
    }
}

fn check_id_for_system_operation(operation: &str) -> HostPrerequisiteCheckId {
    match operation {
        "uname" => HostPrerequisiteCheckId::OsGate,
        "cgroup v2 probe" => HostPrerequisiteCheckId::CgroupMode,
        "user lookup" | "group lookup" => HostPrerequisiteCheckId::JailerIdentity,
        "host prerequisite result construction" => HostPrerequisiteCheckId::HostSubstrateProof,
        _ => HostPrerequisiteCheckId::HostBinaryManifest,
    }
}

fn remediation_for_preflight_error(error: &PreflightError) -> HostPrerequisiteRemediation {
    match error {
        PreflightError::KvmUnavailable { .. } | PreflightError::KvmNotWritable { .. } => {
            HostPrerequisiteRemediation::policy_link(REPAIR_KVM, HOST_SETUP_DOC)
        }
        PreflightError::CgroupV2Unavailable | PreflightError::InvalidCgroupMode { .. } => {
            HostPrerequisiteRemediation::policy_link(REPAIR_CGROUP_MODE, HOST_SETUP_DOC)
        }
        PreflightError::PrivilegeUnavailable { .. } | PreflightError::CapabilityRead(_) => {
            HostPrerequisiteRemediation::policy_link(REPAIR_PRIVILEGE, HOST_SETUP_DOC)
        }
        PreflightError::FirecrackerCveFloorViolation { .. } => {
            HostPrerequisiteRemediation::policy_link(
                REPAIR_UPGRADE_FIRECRACKER_CVE_FLOOR,
                FIRECRACKER_CVE_FLOOR_DOC,
            )
        }
        PreflightError::FirecrackerBinaryNotFound { .. }
        | PreflightError::FirecrackerVersionMismatch { .. }
        | PreflightError::FirecrackerVersionCommandFailed { .. }
        | PreflightError::FirecrackerVersionOutputMalformed { .. }
        | PreflightError::FirecrackerSeccompFilterNotFound { .. }
        | PreflightError::FirecrackerSeccompFilterEmpty { .. }
        | PreflightError::JailerBinaryNotFound { .. }
        | PreflightError::JailerVersionCommandFailed { .. }
        | PreflightError::JailerVersionOutputMalformed { .. }
        | PreflightError::JailerVersionMismatch { .. } => HostPrerequisiteRemediation::policy_link(
            REPAIR_INSTALL_FIRECRACKER_PREREQUISITES,
            HOST_PREREQUISITE_POLICY_DOC,
        ),
        PreflightError::HostBinaryManifest(_)
        | PreflightError::JailerHardenBinaryNotFound { .. }
        | PreflightError::NetHelperBinaryNotFound { .. }
        | PreflightError::HostBinaryMissing { .. }
        | PreflightError::HostBinaryDuplicate { .. }
        | PreflightError::HostBinaryPathMismatch { .. }
        | PreflightError::BinaryHashMismatch { .. }
        | PreflightError::HostBinaryPermission { .. }
        | PreflightError::HostBinaryVersionCommandFailed { .. }
        | PreflightError::HostBinaryVersionMismatch { .. }
        | PreflightError::HostLaunchMaterialMissing { .. }
        | PreflightError::HostLaunchMaterialDuplicate { .. }
        | PreflightError::HostLaunchMaterialPathMismatch { .. }
        | PreflightError::HostLaunchMaterialHashMismatch { .. }
        | PreflightError::HostLaunchMaterialPermission { .. }
        | PreflightError::HostLaunchMaterialVersionMismatch { .. } => {
            HostPrerequisiteRemediation::policy_link(
                REPAIR_HOST_BINARIES_MANIFEST,
                BINARY_INSTALLATION_DOC,
            )
        }
        _ => HostPrerequisiteRemediation::policy_link(REPAIR_HOST_SETUP, HOST_SETUP_DOC),
    }
}

fn apply_preflight_error_fields(check: &mut HostPrerequisiteCheck, error: &PreflightError) {
    match error {
        PreflightError::UnsupportedHostPlatform { actual } => {
            check.expected_value = Some("Linux".to_string());
            check.actual_value = Some(actual.clone());
        }
        PreflightError::HostKernelUnsupported { actual, minimum } => {
            check.expected_version = Some(minimum.clone());
            check.actual_version = Some(actual.clone());
        }
        PreflightError::KvmUnavailable { path } => {
            check.final_path = Some(path.clone());
            check.expected_value = Some("present and writable".to_string());
            check.actual_value = Some("missing".to_string());
        }
        PreflightError::KvmNotWritable { path } => {
            check.final_path = Some(path.clone());
            check.expected_value = Some("present and writable".to_string());
            check.actual_value = Some("not writable".to_string());
        }
        PreflightError::KvmCpuExtensionMissing => {
            check.expected_value = Some("vmx or svm".to_string());
            check.actual_value = Some("missing".to_string());
        }
        PreflightError::InvalidCgroupMode { actual } => {
            check.expected_value = Some("unified-v2 or disabled".to_string());
            check.actual_value = Some(actual.clone());
        }
        PreflightError::CgroupV2Unavailable => {
            check.expected_value = Some("unified cgroup v2".to_string());
            check.actual_value = Some("unavailable".to_string());
        }
        PreflightError::InvalidJailIdentity { field, value } => {
            check.expected_value = Some(format!("{field} u32"));
            check.actual_value = Some(value.clone());
        }
        PreflightError::JailIdentityUnavailable { field, id } => {
            check.expected_value = Some(format!("{field} present in host identity database"));
            check.actual_value = Some(format!("{field} id {id} not found"));
        }
        PreflightError::CpuVulnerabilityDetected { id, detail } => {
            check.expected_value = Some(format!("{id} mitigated"));
            check.actual_value = Some(detail.clone());
        }
        PreflightError::VsockUnavailable => {
            check.expected_value = Some("vhost-vsock available".to_string());
            check.actual_value = Some("unavailable".to_string());
        }
        PreflightError::TunUnavailable => {
            check.expected_value = Some("tun available".to_string());
            check.actual_value = Some("unavailable".to_string());
        }
        PreflightError::NfConntrackUnavailable => {
            check.expected_value = Some("nf_conntrack available".to_string());
            check.actual_value = Some("unavailable".to_string());
        }
        PreflightError::BridgeNetfilterUnavailable => {
            check.expected_value = Some("br_netfilter available".to_string());
            check.actual_value = Some("unavailable".to_string());
        }
        PreflightError::BridgeNfCallIptablesDisabled { actual } => {
            check.expected_value = Some("1".to_string());
            check.actual_value = Some(actual.clone());
        }
        PreflightError::NfConntrackCapacityTooLow {
            actual,
            minimum,
            expected_concurrent_vms,
        } => {
            check.expected_value = Some(format!(
                ">= {minimum} for {expected_concurrent_vms} concurrent VMs"
            ));
            check.actual_value = Some(actual.to_string());
        }
        PreflightError::InvalidNfConntrackMax { actual } => {
            check.expected_value = Some("u64".to_string());
            check.actual_value = Some(actual.clone());
        }
        PreflightError::InvalidExpectedConcurrentVms { actual } => {
            check.expected_value = Some("positive u32".to_string());
            check.actual_value = Some(actual.clone());
        }
        PreflightError::KernelModulesMissing { missing } => {
            check.expected_value = Some("required kernel modules loaded".to_string());
            check.actual_value = Some(format!("missing {}", missing.join(",")));
        }
        PreflightError::KsmEnabled { actual } => {
            check.final_path = Some("/sys/kernel/mm/ksm/run".into());
            check.expected_value = Some("0".to_string());
            check.actual_value = Some(actual.clone());
        }
        PreflightError::SmtEnabled { actual } => {
            check.final_path = Some("/sys/devices/system/cpu/smt/control".into());
            check.expected_value = Some("off".to_string());
            check.actual_value = Some(actual.clone());
        }
        PreflightError::SwapActive { devices } => {
            check.final_path = Some("/proc/swaps".into());
            check.expected_value = Some("header only".to_string());
            check.actual_value = Some(devices.join(","));
        }
        PreflightError::NestedVirtEnabled { vendor } => {
            check.expected_value = Some("N or 0".to_string());
            check.actual_value = Some(format!("{vendor} nested enabled"));
        }
        PreflightError::PrivilegeUnavailable { missing_caps } => {
            check.expected_value = Some("root or required capabilities".to_string());
            check.actual_value = Some(format!("missing {missing_caps:?}"));
        }
        PreflightError::CapabilityRead(source) => {
            check.expected_value = Some("capability state readable".to_string());
            check.actual_value = Some(source.to_string());
        }
        PreflightError::FirecrackerBinaryNotFound { path }
        | PreflightError::JailerBinaryNotFound { path }
        | PreflightError::JailerHardenBinaryNotFound { path }
        | PreflightError::NetHelperBinaryNotFound { path }
        | PreflightError::FirecrackerSeccompFilterNotFound { path } => {
            check.final_path = Some(path.clone());
            check.expected_value = Some("present".to_string());
            check.actual_value = Some("missing".to_string());
        }
        PreflightError::FirecrackerSeccompFilterEmpty { path } => {
            check.final_path = Some(path.clone());
            check.expected_value = Some("non-empty regular file".to_string());
            check.actual_value = Some("empty file".to_string());
        }
        PreflightError::FirecrackerVersionCommandFailed { path, status }
        | PreflightError::JailerVersionCommandFailed { path, status }
        | PreflightError::HostBinaryVersionCommandFailed { path, status, .. } => {
            check.final_path = Some(path.clone());
            check.expected_value = Some("version command exits 0".to_string());
            check.actual_value = Some(status.clone());
        }
        PreflightError::FirecrackerVersionMismatch {
            expected, actual, ..
        }
        | PreflightError::JailerVersionMismatch {
            expected, actual, ..
        } => {
            check.expected_version = Some(expected.clone());
            check.actual_version = Some(actual.clone());
        }
        PreflightError::FirecrackerCveFloorViolation {
            expected, actual, ..
        } => {
            check.expected_version = Some(expected.clone());
            check.actual_version = Some(actual.clone());
        }
        PreflightError::FirecrackerVersionOutputMalformed { actual, .. }
        | PreflightError::JailerVersionOutputMalformed { actual, .. } => {
            check.expected_value = Some("official release version output".to_string());
            check.actual_value = Some(actual.clone());
            check.actual_version = Some(actual.clone());
        }
        PreflightError::HostBinaryManifest(source) => {
            check.expected_value = Some("host-binaries manifest parses and validates".to_string());
            check.actual_value = Some(source.to_string());
        }
        PreflightError::HostBinaryMissing { name }
        | PreflightError::HostLaunchMaterialMissing { name } => {
            check.expected_value = Some(format!("{name} manifest entry present"));
            check.actual_value = Some("missing".to_string());
        }
        PreflightError::HostBinaryDuplicate { name }
        | PreflightError::HostLaunchMaterialDuplicate { name } => {
            check.expected_value = Some(format!("single {name} manifest entry"));
            check.actual_value = Some("duplicate".to_string());
        }
        PreflightError::HostBinaryPathMismatch {
            expected, actual, ..
        }
        | PreflightError::HostLaunchMaterialPathMismatch {
            expected, actual, ..
        } => {
            check.final_path = Some(actual.clone());
            check.expected_value = Some(expected.display().to_string());
            check.actual_value = Some(actual.display().to_string());
        }
        PreflightError::BinaryHashMismatch {
            path,
            expected,
            actual,
            ..
        }
        | PreflightError::HostLaunchMaterialHashMismatch {
            path,
            expected,
            actual,
            ..
        } => {
            check.final_path = Some(path.clone());
            check.expected_sha256 = Some(expected.clone());
            check.actual_sha256 = Some(actual.clone());
        }
        PreflightError::HostBinaryVersionMismatch {
            path,
            expected,
            actual,
            ..
        }
        | PreflightError::HostLaunchMaterialVersionMismatch {
            path,
            expected,
            actual,
            ..
        } => {
            check.final_path = Some(path.clone());
            check.expected_version = Some(expected.clone());
            check.actual_version = Some(actual.clone());
        }
        PreflightError::HostBinaryPermission { path, reason, .. }
        | PreflightError::HostLaunchMaterialPermission { path, reason, .. } => {
            check.final_path = Some(path.clone());
            check.expected_value = Some("root-owned non-writable safe path".to_string());
            check.actual_value = Some((*reason).to_string());
        }
        PreflightError::NonAbsolutePath { path, .. } => {
            check.final_path = Some(path.clone());
            check.expected_value = Some("absolute path".to_string());
            check.actual_value = Some(path.display().to_string());
        }
        PreflightError::PathIo { path, source } => {
            check.final_path = Some(path.clone());
            check.expected_value = Some("filesystem operation succeeds".to_string());
            check.actual_value = Some(source.to_string());
        }
        PreflightError::SystemIo { operation, source } => {
            check.expected_value = Some(format!("{operation} succeeds"));
            check.actual_value = Some(source.to_string());
        }
        _ => {}
    }
}

/// Unix uid/gid owner fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostPrerequisiteOwner {
    /// Numeric uid.
    pub uid: u32,
    /// Numeric gid.
    pub gid: u32,
}

/// Pass/fail status for one prerequisite check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostPrerequisiteStatus {
    /// Check passed.
    Pass,
    /// Check failed and must carry failure/remediation fields.
    Fail,
}

/// Remediation attached to a failing host-prerequisite check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostPrerequisiteRemediation {
    /// Stable remediation token.
    pub id: String,
    /// Exact command to run, when m80 can name one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// Policy or runbook link, when the fix is operator-specific.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy_link: Option<String>,
}

impl HostPrerequisiteRemediation {
    /// Build remediation that points at a command.
    #[must_use]
    pub fn command(id: impl Into<String>, command: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            command: Some(command.into()),
            policy_link: None,
        }
    }

    /// Build remediation that points at a policy/runbook.
    #[must_use]
    pub fn policy_link(id: impl Into<String>, policy_link: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            command: None,
            policy_link: Some(policy_link.into()),
        }
    }
}

/// Host-prerequisite result decode or validation error.
#[derive(Debug, thiserror::Error)]
pub enum HostPrerequisiteResultError {
    /// JSON did not match the schema.
    #[error("host prerequisite result json: {0}")]
    Json(#[from] serde_json::Error),
    /// Schema version is unsupported.
    #[error(
        "unsupported host prerequisite result schema version: expected {expected}, got {actual}"
    )]
    UnsupportedSchemaVersion {
        /// Current supported schema version.
        expected: u32,
        /// Actual decoded schema version.
        actual: u32,
    },
    /// Failed check did not carry a typed failure variant.
    #[error("host prerequisite check {check_name:?} missing failure variant")]
    MissingFailureVariant {
        /// Check name.
        check_name: String,
    },
    /// Failed check did not carry a stable remediation token.
    #[error("host prerequisite check {check_name:?} missing remediation token")]
    MissingRemediationToken {
        /// Check name.
        check_name: String,
    },
    /// Failed check did not carry a command or policy link.
    #[error("host prerequisite check {check_name:?} missing remediation command or policy link")]
    MissingRemediationTarget {
        /// Check name.
        check_name: String,
    },
    /// Success-row projection received a failed table row.
    #[error("host prerequisite success row {check_name:?} was marked failed")]
    UnexpectedFailedSuccessRow {
        /// Check name.
        check_name: String,
    },
    /// Full report is missing an expected check id.
    #[error(
        "host prerequisite full report missing check_id {check_id:?} at index {expected_index}"
    )]
    MissingCheckId {
        /// Missing check id.
        check_id: &'static str,
        /// Expected registry index.
        expected_index: usize,
    },
    /// Full report repeated a check id.
    #[error(
        "host prerequisite full report duplicate check_id {check_id:?}: first index {first_index}, duplicate index {duplicate_index}"
    )]
    DuplicateCheckId {
        /// Duplicated check id.
        check_id: &'static str,
        /// First observed index.
        first_index: usize,
        /// Repeated observed index.
        duplicate_index: usize,
    },
    /// Full report emitted a check id at the wrong registry position.
    #[error(
        "host prerequisite full report out-of-order check_id at index {index}: expected {expected_check_id:?}, got {actual_check_id:?}"
    )]
    OutOfOrderCheckId {
        /// Expected check id at this index.
        expected_check_id: &'static str,
        /// Actual observed check id at this index.
        actual_check_id: &'static str,
        /// Mismatched index.
        index: usize,
    },
}
