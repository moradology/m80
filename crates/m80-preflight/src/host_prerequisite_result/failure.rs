use serde::{Deserialize, Serialize};

use crate::PreflightError;

/// Stable failure kinds recognized by the host-prerequisite result schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostPrerequisiteFailureKind {
    /// Host kernel is not Linux.
    UnsupportedHostPlatform,
    /// Host Linux kernel is older than the supported floor.
    HostKernelUnsupported,
    /// `/dev/kvm` is missing.
    KvmUnavailable,
    /// `/dev/kvm` exists but is not writable.
    KvmNotWritable,
    /// Host CPU does not expose KVM virtualization extensions.
    KvmCpuExtensionMissing,
    /// Configured cgroup mode is invalid.
    InvalidCgroupMode,
    /// Configured jail identity is invalid.
    InvalidJailIdentity,
    /// Configured jail identity is absent from host databases.
    JailIdentityUnavailable,
    /// Host reports an unmitigated CPU vulnerability.
    CpuVulnerabilityDetected,
    /// Unified cgroup v2 is unavailable.
    CgroupV2Unavailable,
    /// vhost-vsock support is unavailable.
    VsockUnavailable,
    /// TUN/TAP support is unavailable.
    TunUnavailable,
    /// nf_conntrack support is unavailable.
    NfConntrackUnavailable,
    /// br_netfilter support is unavailable.
    BridgeNetfilterUnavailable,
    /// Bridge packets are not routed through iptables.
    BridgeNfCallIptablesDisabled,
    /// nf_conntrack capacity is below the configured VM concurrency floor.
    NfConntrackCapacityTooLow,
    /// nf_conntrack capacity sysctl did not parse.
    InvalidNfConntrackMax,
    /// Expected concurrent VM count did not parse.
    InvalidExpectedConcurrentVms,
    /// Required kernel modules are missing.
    KernelModulesMissing,
    /// Kernel samepage merging is enabled.
    KsmEnabled,
    /// Simultaneous multithreading is enabled and configured as hard-fail.
    SmtEnabled,
    /// Host swap has active entries.
    SwapActive,
    /// Nested virtualization is enabled for a KVM vendor module.
    NestedVirtEnabled,
    /// Required privilege is unavailable.
    PrivilegeUnavailable,
    /// Capability read failed.
    CapabilityRead,
    /// Firecracker binary is missing.
    FirecrackerBinaryNotFound,
    /// Firecracker version did not match policy.
    FirecrackerVersionMismatch,
    /// Firecracker version command failed.
    FirecrackerVersionCommandFailed,
    /// Firecracker version output was malformed.
    FirecrackerVersionOutputMalformed,
    /// Firecracker version violates an active CVE floor.
    FirecrackerCveFloorViolation,
    /// Firecracker seccomp filter is missing.
    FirecrackerSeccompFilterNotFound,
    /// Firecracker seccomp filter is empty.
    FirecrackerSeccompFilterEmpty,
    /// Jailer binary is missing.
    JailerBinaryNotFound,
    /// Jailer version command failed.
    JailerVersionCommandFailed,
    /// Jailer version output was malformed.
    JailerVersionOutputMalformed,
    /// Jailer version did not match Firecracker.
    JailerVersionMismatch,
    /// m80 jailer hardening wrapper is missing.
    JailerHardenBinaryNotFound,
    /// m80 network helper is missing.
    NetHelperBinaryNotFound,
    /// Host binary manifest failed schema or I/O validation.
    HostBinaryManifest,
    /// Required host binary entry is missing.
    HostBinaryMissing,
    /// Host binary entry is duplicated.
    HostBinaryDuplicate,
    /// Host binary path differs from manifest.
    HostBinaryPathMismatch,
    /// Host binary hash differs from manifest.
    BinaryHashMismatch,
    /// Host binary permissions are unsafe.
    HostBinaryPermission,
    /// Host binary version command failed.
    HostBinaryVersionCommandFailed,
    /// Host binary version differs from manifest.
    HostBinaryVersionMismatch,
    /// Required host launch material is missing.
    HostLaunchMaterialMissing,
    /// Host launch material entry is duplicated.
    HostLaunchMaterialDuplicate,
    /// Host launch material path differs from manifest.
    HostLaunchMaterialPathMismatch,
    /// Host launch material hash differs from manifest.
    HostLaunchMaterialHashMismatch,
    /// Host launch material permissions are unsafe.
    HostLaunchMaterialPermission,
    /// Host launch material version differs from manifest.
    HostLaunchMaterialVersionMismatch,
    /// Host path is not absolute.
    NonAbsolutePath,
    /// Filesystem I/O failed for a known path.
    PathIo,
    /// Host syscall or command failed.
    SystemIo,
}

impl HostPrerequisiteFailureKind {
    /// Map a preflight error to a host-prerequisite failure kind, when the
    /// error belongs to the host-prerequisite contract.
    #[must_use]
    pub fn from_preflight_error(error: &PreflightError) -> Option<Self> {
        Some(match error {
            PreflightError::UnsupportedHostPlatform { .. } => Self::UnsupportedHostPlatform,
            PreflightError::HostKernelUnsupported { .. } => Self::HostKernelUnsupported,
            PreflightError::KvmUnavailable { .. } => Self::KvmUnavailable,
            PreflightError::KvmNotWritable { .. } => Self::KvmNotWritable,
            PreflightError::KvmCpuExtensionMissing => Self::KvmCpuExtensionMissing,
            PreflightError::InvalidCgroupMode { .. } => Self::InvalidCgroupMode,
            PreflightError::InvalidJailIdentity { .. } => Self::InvalidJailIdentity,
            PreflightError::JailIdentityUnavailable { .. } => Self::JailIdentityUnavailable,
            PreflightError::CpuVulnerabilityDetected { .. } => Self::CpuVulnerabilityDetected,
            PreflightError::CgroupV2Unavailable => Self::CgroupV2Unavailable,
            PreflightError::VsockUnavailable => Self::VsockUnavailable,
            PreflightError::TunUnavailable => Self::TunUnavailable,
            PreflightError::NfConntrackUnavailable => Self::NfConntrackUnavailable,
            PreflightError::BridgeNetfilterUnavailable => Self::BridgeNetfilterUnavailable,
            PreflightError::BridgeNfCallIptablesDisabled { .. } => {
                Self::BridgeNfCallIptablesDisabled
            }
            PreflightError::NfConntrackCapacityTooLow { .. } => Self::NfConntrackCapacityTooLow,
            PreflightError::InvalidNfConntrackMax { .. } => Self::InvalidNfConntrackMax,
            PreflightError::InvalidExpectedConcurrentVms { .. } => {
                Self::InvalidExpectedConcurrentVms
            }
            PreflightError::KernelModulesMissing { .. } => Self::KernelModulesMissing,
            PreflightError::KsmEnabled { .. } => Self::KsmEnabled,
            PreflightError::SmtEnabled { .. } => Self::SmtEnabled,
            PreflightError::SwapActive { .. } => Self::SwapActive,
            PreflightError::NestedVirtEnabled { .. } => Self::NestedVirtEnabled,
            PreflightError::PrivilegeUnavailable { .. } => Self::PrivilegeUnavailable,
            PreflightError::CapabilityRead(_) => Self::CapabilityRead,
            PreflightError::FirecrackerBinaryNotFound { .. } => Self::FirecrackerBinaryNotFound,
            PreflightError::FirecrackerVersionMismatch { .. } => Self::FirecrackerVersionMismatch,
            PreflightError::FirecrackerVersionCommandFailed { .. } => {
                Self::FirecrackerVersionCommandFailed
            }
            PreflightError::FirecrackerVersionOutputMalformed { .. } => {
                Self::FirecrackerVersionOutputMalformed
            }
            PreflightError::FirecrackerCveFloorViolation { .. } => {
                Self::FirecrackerCveFloorViolation
            }
            PreflightError::FirecrackerSeccompFilterNotFound { .. } => {
                Self::FirecrackerSeccompFilterNotFound
            }
            PreflightError::FirecrackerSeccompFilterEmpty { .. } => {
                Self::FirecrackerSeccompFilterEmpty
            }
            PreflightError::JailerBinaryNotFound { .. } => Self::JailerBinaryNotFound,
            PreflightError::JailerVersionCommandFailed { .. } => Self::JailerVersionCommandFailed,
            PreflightError::JailerVersionOutputMalformed { .. } => {
                Self::JailerVersionOutputMalformed
            }
            PreflightError::JailerVersionMismatch { .. } => Self::JailerVersionMismatch,
            PreflightError::JailerHardenBinaryNotFound { .. } => Self::JailerHardenBinaryNotFound,
            PreflightError::NetHelperBinaryNotFound { .. } => Self::NetHelperBinaryNotFound,
            PreflightError::HostBinaryManifest(_) => Self::HostBinaryManifest,
            PreflightError::HostBinaryMissing { .. } => Self::HostBinaryMissing,
            PreflightError::HostBinaryDuplicate { .. } => Self::HostBinaryDuplicate,
            PreflightError::HostBinaryPathMismatch { .. } => Self::HostBinaryPathMismatch,
            PreflightError::BinaryHashMismatch { .. } => Self::BinaryHashMismatch,
            PreflightError::HostBinaryPermission { .. } => Self::HostBinaryPermission,
            PreflightError::HostBinaryVersionCommandFailed { .. } => {
                Self::HostBinaryVersionCommandFailed
            }
            PreflightError::HostBinaryVersionMismatch { .. } => Self::HostBinaryVersionMismatch,
            PreflightError::HostLaunchMaterialMissing { .. } => Self::HostLaunchMaterialMissing,
            PreflightError::HostLaunchMaterialDuplicate { .. } => Self::HostLaunchMaterialDuplicate,
            PreflightError::HostLaunchMaterialPathMismatch { .. } => {
                Self::HostLaunchMaterialPathMismatch
            }
            PreflightError::HostLaunchMaterialHashMismatch { .. } => {
                Self::HostLaunchMaterialHashMismatch
            }
            PreflightError::HostLaunchMaterialPermission { .. } => {
                Self::HostLaunchMaterialPermission
            }
            PreflightError::HostLaunchMaterialVersionMismatch { .. } => {
                Self::HostLaunchMaterialVersionMismatch
            }
            PreflightError::NonAbsolutePath { .. } => Self::NonAbsolutePath,
            PreflightError::PathIo { .. } => Self::PathIo,
            PreflightError::SystemIo { .. } => Self::SystemIo,
            _ => return None,
        })
    }
}
