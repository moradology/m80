use serde::{Deserialize, Serialize};

/// Stable machine identity for a host-prerequisite check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostPrerequisiteCheckId {
    /// Host OS gate.
    OsGate,
    /// Host Linux kernel floor.
    HostKernelFloor,
    /// `/dev/kvm` availability and writability.
    Kvm,
    /// Requested cgroup mode support.
    CgroupMode,
    /// Configured jailer UID/GID identity.
    JailerIdentity,
    /// Root or capability-bearing privilege.
    Privilege,
    /// Proof-kind marker separating live preflight from hostless fixture proof.
    HostSubstrateProof,
    /// CPU virtualization extension check.
    KvmCpuExtensions,
    /// Required host kernel module check.
    KernelModules,
    /// Transparent hugepage advisory check.
    TransparentHugepages,
    /// KVM halt-polling advisory check.
    KvmHaltPolling,
    /// CPU governor advisory check.
    CpuGovernor,
    /// CPU microcode observation check.
    CpuMicrocode,
    /// CPU vulnerability observation check.
    CpuVulnerabilities,
    /// nf_conntrack capacity check.
    ConntrackCapacity,
    /// Firecracker binary identity check.
    FirecrackerBinary,
    /// Firecracker seccomp filter identity check.
    FirecrackerSeccompFilter,
    /// Official jailer binary identity check.
    JailerBinary,
    /// m80 jailer hardening wrapper identity check.
    JailerHardeningWrapper,
    /// m80 network helper identity check.
    NetworkHelper,
    /// Generated host-binaries manifest identity check.
    HostBinaryManifest,
    /// Kernel image artifact check.
    KernelImage,
    /// Rootfs and guest manifest artifact check.
    RootfsManifest,
    /// Run-root directory check.
    RunRoot,
    /// Run-root filesystem capability advisory.
    RunRootFilesystem,
    /// Host storage helper availability check.
    StorageHelpers,
}

impl HostPrerequisiteCheckId {
    /// Stable registry order used by docs and contract tests.
    pub const ALL: &'static [Self] = &[
        Self::OsGate,
        Self::HostKernelFloor,
        Self::Kvm,
        Self::CgroupMode,
        Self::JailerIdentity,
        Self::Privilege,
        Self::HostSubstrateProof,
        Self::KvmCpuExtensions,
        Self::KernelModules,
        Self::TransparentHugepages,
        Self::KvmHaltPolling,
        Self::CpuGovernor,
        Self::CpuMicrocode,
        Self::CpuVulnerabilities,
        Self::ConntrackCapacity,
        Self::FirecrackerBinary,
        Self::FirecrackerSeccompFilter,
        Self::JailerBinary,
        Self::JailerHardeningWrapper,
        Self::NetworkHelper,
        Self::HostBinaryManifest,
        Self::KernelImage,
        Self::RootfsManifest,
        Self::RunRoot,
        Self::RunRootFilesystem,
        Self::StorageHelpers,
    ];

    /// Default human label for this stable check identity.
    #[must_use]
    pub const fn check_name(self) -> &'static str {
        match self {
            Self::OsGate => "OS gate",
            Self::HostKernelFloor => "Host kernel floor",
            Self::Kvm => "KVM",
            Self::CgroupMode => "Cgroup mode",
            Self::JailerIdentity => "Jailer identity",
            Self::Privilege => "Privilege",
            Self::HostSubstrateProof => "Host substrate proof",
            Self::KvmCpuExtensions => "KVM CPU extensions",
            Self::KernelModules => "Kernel modules",
            Self::TransparentHugepages => "Transparent hugepages",
            Self::KvmHaltPolling => "KVM halt polling",
            Self::CpuGovernor => "CPU governor",
            Self::CpuMicrocode => "CPU microcode",
            Self::CpuVulnerabilities => "CPU vulnerabilities",
            Self::ConntrackCapacity => "Conntrack capacity",
            Self::FirecrackerBinary => "Firecracker binary",
            Self::FirecrackerSeccompFilter => "Firecracker seccomp filter",
            Self::JailerBinary => "Jailer binary",
            Self::JailerHardeningWrapper => "Jailer hardening wrapper",
            Self::NetworkHelper => "Network helper",
            Self::HostBinaryManifest => "Host binary manifest",
            Self::KernelImage => "Kernel image",
            Self::RootfsManifest => "Rootfs + manifest",
            Self::RunRoot => "Run-root",
            Self::RunRootFilesystem => "Run-root filesystem",
            Self::StorageHelpers => "Storage helpers",
        }
    }

    /// Stable serialized check identity.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OsGate => "os_gate",
            Self::HostKernelFloor => "host_kernel_floor",
            Self::Kvm => "kvm",
            Self::CgroupMode => "cgroup_mode",
            Self::JailerIdentity => "jailer_identity",
            Self::Privilege => "privilege",
            Self::HostSubstrateProof => "host_substrate_proof",
            Self::KvmCpuExtensions => "kvm_cpu_extensions",
            Self::KernelModules => "kernel_modules",
            Self::TransparentHugepages => "transparent_hugepages",
            Self::KvmHaltPolling => "kvm_halt_polling",
            Self::CpuGovernor => "cpu_governor",
            Self::CpuMicrocode => "cpu_microcode",
            Self::CpuVulnerabilities => "cpu_vulnerabilities",
            Self::ConntrackCapacity => "conntrack_capacity",
            Self::FirecrackerBinary => "firecracker_binary",
            Self::FirecrackerSeccompFilter => "firecracker_seccomp_filter",
            Self::JailerBinary => "jailer_binary",
            Self::JailerHardeningWrapper => "jailer_hardening_wrapper",
            Self::NetworkHelper => "network_helper",
            Self::HostBinaryManifest => "host_binary_manifest",
            Self::KernelImage => "kernel_image",
            Self::RootfsManifest => "rootfs_manifest",
            Self::RunRoot => "run_root",
            Self::RunRootFilesystem => "run_root_filesystem",
            Self::StorageHelpers => "storage_helpers",
        }
    }
}
