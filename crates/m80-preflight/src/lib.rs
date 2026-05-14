//! Host capability checks and binary discovery before VM launch.
//!
//! See `README.md` for the black-box contract.
//! Behavior captures: bead epic `m80-a7g` (`br show m80-a7g`).

#![deny(missing_docs)]

use std::fs::File;
use std::io;
use std::os::fd::AsRawFd;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use caps::Capability;
use caps::CapsHashSet;
use serde::{Deserialize, Serialize};

use m80_image_manifest::{BuildReceiptArtifactKind, Manifest, ManifestError};

mod artifacts;
mod binary;
mod cache;
mod checks;
mod cve_floor;
mod table;

/// Linux capabilities m80 needs when `euid != 0`; a process holding all of
/// these passes the privilege precondition without being root. The
/// `setcap` invocation that grants them is in [`PreflightError::PrivilegeUnavailable`]'s
/// hint text.
pub const REQUIRED_CAPABILITIES: &[Capability] = &[
    Capability::CAP_NET_ADMIN,
    Capability::CAP_SYS_ADMIN,
    Capability::CAP_MKNOD,
    Capability::CAP_CHOWN,
    Capability::CAP_FOWNER,
    Capability::CAP_KILL,
    Capability::CAP_SETUID,
    Capability::CAP_SETGID,
    Capability::CAP_SETPCAP,
];

/// Classify process privilege from euid and effective Linux capabilities.
pub fn classify_privilege(
    euid: u32,
    effective: &CapsHashSet,
) -> Result<PrivilegeStatus, PreflightError> {
    if euid == 0 {
        return Ok(PrivilegeStatus::Root);
    }

    let missing_caps: Vec<_> = REQUIRED_CAPABILITIES
        .iter()
        .filter(|cap| !effective.contains(cap))
        .copied()
        .collect();

    if !missing_caps.is_empty() {
        return Err(PreflightError::PrivilegeUnavailable { missing_caps });
    }

    Ok(PrivilegeStatus::CapabilityBearing)
}

/// How m80 has its required privilege on this host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivilegeStatus {
    /// `geteuid() == 0`. Process can perform any privileged syscall.
    Root,
    /// `geteuid() != 0` but the effective capability set contains every
    /// capability in [`REQUIRED_CAPABILITIES`].
    CapabilityBearing,
}

/// One row in the preflight report.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckRow {
    /// Short label naming the check.
    pub label: String,
    /// Whether the check passed.
    pub passed: bool,
    /// Free-text detail rendered alongside the label.
    pub detail: String,
}

/// Output of a successful preflight: every resolved path, version, and
/// capability the rest of the system needs.
#[derive(Debug, Clone)]
pub struct Discovery {
    /// Resolved firecracker binary path.
    pub firecracker_bin: PathBuf,
    /// Resolved Firecracker advanced seccomp filter path.
    pub firecracker_seccomp_filter: PathBuf,
    /// Resolved jailer binary path.
    pub jailer_bin: PathBuf,
    /// Resolved m80 jailer hardening wrapper path.
    pub jailer_harden_bin: PathBuf,
    /// Resolved m80 network helper path.
    pub net_helper_bin: PathBuf,
    /// Resolved kernel image path.
    pub kernel: PathBuf,
    /// Resolved rootfs image path.
    pub rootfs: PathBuf,
    /// Open rootfs file descriptor whose bytes matched the manifest during
    /// preflight.
    pub pinned_rootfs: PinnedRootfs,
    /// Validated provenance manifest.
    pub manifest: Manifest,
    /// Resolved run-root path.
    pub run_root: PathBuf,
    /// How m80 satisfies the privilege precondition on this host.
    pub privilege: PrivilegeStatus,
    /// Per-check rows for the table renderer.
    pub report: Vec<CheckRow>,
}

impl Discovery {
    /// Render `report` as a fixed-width table for human consumption.
    #[must_use]
    pub fn render_table(&self) -> String {
        table::render(&self.report)
    }
}

/// Rootfs artifact opened and held by preflight after sha256 verification.
#[derive(Debug, Clone)]
pub struct PinnedRootfs {
    path: PathBuf,
    file: Arc<File>,
}

impl PinnedRootfs {
    /// Build a pinned rootfs handle from an already-open file.
    ///
    /// The caller is responsible for verifying the file contents before
    /// placing this handle in a [`Discovery`].
    #[must_use]
    pub fn from_file(path: PathBuf, file: File) -> Self {
        Self {
            path,
            file: Arc::new(file),
        }
    }

    /// Original resolved rootfs path used for diagnostics and identity files.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Process-qualified procfs path for this pinned rootfs descriptor.
    ///
    /// The path uses `/proc/<pid>/fd/<fd>` rather than `/proc/self/fd/<fd>`
    /// because launch passes it through subprocess and mount-planning
    /// boundaries. Consumers can open or bind this path while this handle is
    /// alive without re-resolving the original rootfs pathname.
    #[must_use]
    pub fn proc_fd_path(&self) -> PathBuf {
        PathBuf::from(format!(
            "/proc/{}/fd/{}",
            std::process::id(),
            self.file.as_raw_fd()
        ))
    }
}

pub use artifacts::{ArtifactPreflightConfig, ENV_KERNEL_IMAGE, ENV_KERNEL_KIND, ENV_ROOTFS_IMAGE};
pub use binary::{
    BinaryDiscoveryConfig, DEFAULT_FIRECRACKER_BIN, DEFAULT_FIRECRACKER_SECCOMP_FILTER,
    ENV_FIRECRACKER_BIN, ENV_FIRECRACKER_SECCOMP_FILTER, ENV_FIRECRACKER_VERSION,
};
pub use checks::{run, run_with_configs, CgroupPreflightMode, HostFeaturePreflightConfig};

/// Errors surfaced by preflight. `Display` is lowercase, no trailing period,
/// no embedded hint text. Actionable hints live in [`PreflightError::hint`].
#[derive(Debug, thiserror::Error)]
pub enum PreflightError {
    /// Host kernel is not Linux.
    #[error("unsupported host platform: {actual}")]
    UnsupportedHostPlatform {
        /// Platform reported by `uname -s`.
        actual: String,
    },

    /// Host Linux kernel is older than m80's supported floor.
    #[error("host kernel unsupported: actual {actual}, minimum {minimum}")]
    HostKernelUnsupported {
        /// Kernel release reported by `uname -r`.
        actual: String,
        /// Minimum supported `major.minor` release.
        minimum: String,
    },

    /// `/dev/kvm` is missing.
    #[error("kvm device unavailable at {}", path.display())]
    KvmUnavailable {
        /// KVM device path that was missing.
        path: PathBuf,
    },

    /// `/dev/kvm` exists but is not writable.
    #[error("kvm device is not writable at {}", path.display())]
    KvmNotWritable {
        /// KVM device path that rejected write access.
        path: PathBuf,
    },

    /// `/proc/cpuinfo` does not advertise hardware virtualization support.
    #[error("kvm cpu extension missing: expected vmx or svm in /proc/cpuinfo")]
    KvmCpuExtensionMissing,

    /// `M80_CGROUP_MODE` carried a value preflight does not understand.
    #[error("invalid cgroup mode: {actual:?}")]
    InvalidCgroupMode {
        /// Observed value.
        actual: String,
    },

    /// A configured jailer UID/GID env or config value was not a valid u32.
    #[error("invalid jail identity {field}: {value:?}")]
    InvalidJailIdentity {
        /// Field or env key carrying the invalid value.
        field: &'static str,
        /// Observed value.
        value: String,
    },

    /// Configured jailer UID/GID is not present in host identity databases.
    #[error("jail identity unavailable: {field} id {id} not found")]
    JailIdentityUnavailable {
        /// Identity field that failed (`jail_uid` or `jail_gid`).
        field: &'static str,
        /// Missing numeric id.
        id: u32,
    },

    /// A high-impact CPU vulnerability sysfs row reports `Vulnerable` and
    /// m80 has not been explicitly told to skip this host gate.
    #[error("cpu vulnerability {id} is unmitigated: {detail}")]
    CpuVulnerabilityDetected {
        /// Vulnerability file name under `/sys/devices/system/cpu/vulnerabilities`.
        id: String,
        /// Kernel-provided status text.
        detail: String,
    },

    /// `M80_KERNEL_KIND` carried a value preflight does not understand.
    #[error("invalid kernel kind: {actual:?}")]
    InvalidKernelKind {
        /// Observed value.
        actual: String,
    },

    /// Host is not in unified cgroup v2 mode but cgroup v2 was requested.
    #[error("cgroup v2 unavailable")]
    CgroupV2Unavailable,

    /// Host vhost-vsock support is absent. Firecracker needs this for the
    /// vsock device that carries m80's host↔guest control protocol.
    #[error("vhost-vsock unavailable")]
    VsockUnavailable,

    /// Host TUN support is absent. m80 needs `/dev/net/tun` or the `tun`
    /// module for TAP-backed outbound networking.
    #[error("tun unavailable")]
    TunUnavailable,

    /// Host nf_conntrack support is absent. Outbound NAT needs conntrack for
    /// stateful masquerade rules.
    #[error("nf_conntrack unavailable")]
    NfConntrackUnavailable,

    /// Host br_netfilter support is absent. Outbound NAT needs bridge packets
    /// to traverse iptables so TAP-scoped rules are enforceable.
    #[error("br_netfilter unavailable")]
    BridgeNetfilterUnavailable,

    /// Bridge netfilter is present but bridge packets are not routed through
    /// iptables.
    #[error("bridge-nf-call-iptables disabled: actual {actual:?}")]
    BridgeNfCallIptablesDisabled {
        /// Observed sysctl value.
        actual: String,
    },

    /// `net.netfilter.nf_conntrack_max` was below m80's expected concurrency
    /// floor.
    #[error(
        "nf_conntrack_max too low: actual {actual}, minimum {minimum} for {expected_concurrent_vms} expected concurrent VMs"
    )]
    NfConntrackCapacityTooLow {
        /// Current host sysctl value.
        actual: u64,
        /// Required floor for the configured concurrency.
        minimum: u64,
        /// Expected concurrent VMs used to compute the floor.
        expected_concurrent_vms: u32,
    },

    /// The host's `nf_conntrack_max` sysctl did not parse as an integer.
    #[error("invalid nf_conntrack_max: {actual:?}")]
    InvalidNfConntrackMax {
        /// Observed sysctl text.
        actual: String,
    },

    /// `M80_MAX_CONCURRENT_VMS` was not a positive u32 for preflight sizing.
    #[error("invalid expected concurrent VM count: {actual:?}")]
    InvalidExpectedConcurrentVms {
        /// Observed value.
        actual: String,
    },

    /// Required kernel modules are not loaded/loadable.
    #[error("kernel modules missing: {missing:?}")]
    KernelModulesMissing {
        /// Names of the missing modules (e.g., `tap`, `bridge`).
        missing: Vec<String>,
    },

    /// `geteuid() != 0` AND one or more capabilities in
    /// [`REQUIRED_CAPABILITIES`] are absent from the effective set.
    /// Operator must run as root, `setcap` the binary, or grant the caps via
    /// a container `securityContext`.
    #[error("insufficient privilege; missing capabilities: {missing_caps:?}")]
    PrivilegeUnavailable {
        /// Capabilities that were required but not present.
        missing_caps: Vec<Capability>,
    },

    /// `firecracker` not found via env or default path.
    #[error("firecracker binary not found")]
    FirecrackerBinaryNotFound,

    /// `firecracker --version` did not match the configured pin.
    #[error("firecracker version mismatch: expected {expected}, got {actual}")]
    FirecrackerVersionMismatch {
        /// Pinned version (from env).
        expected: String,
        /// Reported version.
        actual: String,
    },

    /// `firecracker --version` exited non-zero.
    #[error("firecracker --version at {} exited with {status}", path.display())]
    FirecrackerVersionCommandFailed {
        /// Binary path that was executed.
        path: PathBuf,
        /// Exit status string.
        status: String,
    },

    /// `firecracker --version` matched a version affected by a documented
    /// security advisory or did not parse as a release version.
    #[error(
        "firecracker version fails CVE floor for {cve_id}: actual {actual}, fixed versions {fixed_versions}"
    )]
    FirecrackerCveFloorViolation {
        /// CVE identifier, or `firecracker-version-format` for malformed versions.
        cve_id: String,
        /// Reported version.
        actual: String,
        /// Documented fixed version set.
        fixed_versions: String,
    },

    /// The configured Firecracker advanced seccomp filter is missing or is
    /// not a regular file.
    #[error("firecracker seccomp filter not found at {}", path.display())]
    FirecrackerSeccompFilterNotFound {
        /// Filter path that failed validation.
        path: PathBuf,
    },

    /// The configured Firecracker advanced seccomp filter file is empty.
    #[error("firecracker seccomp filter is empty at {}", path.display())]
    FirecrackerSeccompFilterEmpty {
        /// Filter path that failed validation.
        path: PathBuf,
    },

    /// `jailer` binary not found.
    #[error("jailer binary not found")]
    JailerBinaryNotFound,

    /// `m80-jailer-harden` binary not found.
    #[error("jailer hardening wrapper not found")]
    JailerHardenBinaryNotFound,

    /// `m80-net-helper` binary not found.
    #[error("network helper binary not found")]
    NetHelperBinaryNotFound,

    /// Host TCB binary manifest read/validate failed.
    #[error("host binary manifest: {0}")]
    HostBinaryManifest(#[source] ManifestError),

    /// A required host TCB binary is absent from `host-binaries.manifest.json`.
    #[error("host binary manifest missing required entry: {name}")]
    HostBinaryMissing {
        /// Required logical binary name.
        name: &'static str,
    },

    /// `host-binaries.manifest.json` contains a duplicate logical binary.
    #[error("host binary manifest duplicate entry: {name}")]
    HostBinaryDuplicate {
        /// Duplicated logical binary name.
        name: &'static str,
    },

    /// A configured host binary path differs from the install-time manifest.
    #[error(
        "host binary path mismatch for {name}: expected {}, got {}",
        expected.display(),
        actual.display()
    )]
    HostBinaryPathMismatch {
        /// Logical binary name.
        name: &'static str,
        /// Runtime-configured path.
        expected: PathBuf,
        /// Manifest-recorded path.
        actual: PathBuf,
    },

    /// A host binary digest changed after install-time recording.
    #[error(
        "host binary sha256 mismatch for {name} at {}: expected {expected}, got {actual}",
        path.display()
    )]
    BinaryHashMismatch {
        /// Logical binary name.
        name: &'static str,
        /// Manifest-recorded path.
        path: PathBuf,
        /// Manifest-recorded digest.
        expected: String,
        /// Recomputed digest.
        actual: String,
    },

    /// A host binary's ownership or mode is unsafe.
    #[error("host binary permission rejected for {name} at {}: {reason}", path.display())]
    HostBinaryPermission {
        /// Logical binary name.
        name: &'static str,
        /// Manifest-recorded path.
        path: PathBuf,
        /// Rejection reason.
        reason: &'static str,
    },

    /// A host artifact path was present but was not absolute.
    #[error("{kind} path is not absolute: {}", path.display())]
    NonAbsolutePath {
        /// Artifact class, e.g. `kernel` or `rootfs`.
        kind: String,
        /// Offending path.
        path: PathBuf,
    },

    /// No `vmlinux-*` discovered.
    #[error("kernel image not found")]
    KernelNotFound,

    /// No rootfs image discovered (or env override missing).
    #[error("rootfs image not found")]
    RootfsNotFound,

    /// Artifact directory permissions allow an untrusted group/world writer to
    /// swap boot artifacts between verification and launch.
    #[error("artifact directory is group/world-writable: {} mode {mode:o}", path.display())]
    ArtifactDirectoryWritable {
        /// Directory whose mode was unsafe.
        path: PathBuf,
        /// Unix permission bits observed on the directory.
        mode: u32,
    },

    /// Rootfs artifact permissions allow an untrusted group/world writer to
    /// mutate the same inode after verification.
    #[error("artifact file is group/world-writable: {} mode {mode:o}", path.display())]
    ArtifactFileWritable {
        /// File whose mode was unsafe.
        path: PathBuf,
        /// Unix permission bits observed on the file.
        mode: u32,
    },

    /// Manifest read/validate failed.
    #[error("manifest: {0}")]
    Manifest(#[from] ManifestError),

    /// Build receipt read/validate failed.
    #[error("build receipt: {0}")]
    BuildReceipt(#[source] ManifestError),

    /// Build receipt points at a different guest manifest than preflight read.
    #[error(
        "build receipt manifest path mismatch: expected {}, got {}",
        expected.display(),
        actual.display()
    )]
    BuildReceiptPathMismatch {
        /// Preflight-selected manifest path.
        expected: PathBuf,
        /// Receipt-recorded manifest path after root-relative resolution.
        actual: PathBuf,
    },

    /// Build receipt's manifest digest does not match the manifest bytes.
    #[error(
        "build receipt manifest sha256 mismatch at {}: expected {expected}, got {actual}",
        path.display()
    )]
    BuildReceiptManifestMismatch {
        /// Manifest path read by preflight.
        path: PathBuf,
        /// Receipt-recorded manifest digest.
        expected: String,
        /// Recomputed manifest digest.
        actual: String,
    },

    /// Build receipt omitted a required artifact kind.
    #[error("build receipt missing artifact: {kind:?}")]
    BuildReceiptArtifactMissing {
        /// Missing artifact kind.
        kind: BuildReceiptArtifactKind,
    },

    /// Build receipt duplicated an artifact kind.
    #[error("build receipt duplicate artifact: {kind:?}")]
    BuildReceiptArtifactDuplicate {
        /// Duplicated artifact kind.
        kind: BuildReceiptArtifactKind,
    },

    /// Build receipt artifact path differs from the guest manifest.
    #[error(
        "build receipt artifact path mismatch for {kind:?}: expected {}, got {}",
        expected.display(),
        actual.display()
    )]
    BuildReceiptArtifactPathMismatch {
        /// Artifact kind.
        kind: BuildReceiptArtifactKind,
        /// Guest manifest artifact path.
        expected: PathBuf,
        /// Receipt artifact path.
        actual: PathBuf,
    },

    /// Build receipt artifact digest differs from the guest manifest.
    #[error(
        "build receipt artifact sha256 mismatch for {kind:?}: expected {expected}, got {actual}"
    )]
    BuildReceiptArtifactHashMismatch {
        /// Artifact kind.
        kind: BuildReceiptArtifactKind,
        /// Guest manifest artifact digest.
        expected: String,
        /// Receipt artifact digest.
        actual: String,
    },

    /// Run-root directory is absent or has insufficient free capacity
    /// (< 100 MiB). This variant covers both "directory does not exist" and
    /// "not enough space" so a single variant name handles both conditions.
    #[error("run-root unavailable: {reason}")]
    RunRootUnavailable {
        /// Human-readable reason (missing directory or insufficient space).
        reason: String,
    },

    /// One of the storage helpers (`mkfs.ext4`, `cp`, `fallocate`, `debugfs`,
    /// `e2fsck`) is missing.
    #[error("storage helper missing on PATH: {0}")]
    StorageHelperMissing(String),

    /// `caps::read()` syscall failed (capability system error, not an I/O
    /// error).
    #[error("capability read failed: {0}")]
    CapabilityRead(#[source] caps::errors::CapsError),

    /// Filesystem I/O failure where the target path is known.
    #[error("i/o on {}: {source}", path.display())]
    PathIo {
        /// Path the operation targeted.
        path: PathBuf,
        /// Underlying I/O error.
        #[source]
        source: io::Error,
    },

    /// Host syscall or command execution failed without a single filesystem
    /// target path.
    #[error("{operation} failed: {source}")]
    SystemIo {
        /// Operation being attempted.
        operation: &'static str,
        /// Underlying I/O error.
        #[source]
        source: io::Error,
    },
}

impl PreflightError {
    /// Actionable operator hint for this error.
    ///
    /// Rendered by the CLI in non-JSON mode as a `hint:` line after the error
    /// message; omitted from the JSON `detail` field so machine readers see a
    /// clean error string.
    #[must_use]
    pub fn hint(&self) -> &'static str {
        match self {
            Self::UnsupportedHostPlatform { .. } => {
                "m80 requires a Linux host; macOS and Windows are not supported"
            }
            Self::HostKernelUnsupported { .. } => {
                "run m80 on a Linux host with kernel 6.1 or newer"
            }
            Self::KvmUnavailable { .. } => {
                "ensure KVM is enabled in the host kernel and /dev/kvm exists"
            }
            Self::KvmNotWritable { .. } => {
                "add your user to the `kvm` group (`sudo usermod -aG kvm $USER`) or run m80 as root"
            }
            Self::KvmCpuExtensionMissing => {
                "enable hardware virtualization in firmware/BIOS and ensure the host CPU exposes vmx or svm"
            }
            Self::InvalidCgroupMode { .. } => {
                "set M80_CGROUP_MODE to either `unified-v2` or `disabled`"
            }
            Self::InvalidJailIdentity { .. } => {
                "set M80_JAIL_UID and M80_JAIL_GID to decimal u32 ids"
            }
            Self::JailIdentityUnavailable { .. } => {
                "create the configured jail user/group on the host or set M80_JAIL_UID/M80_JAIL_GID to existing ids"
            }
            Self::CpuVulnerabilityDetected { .. } => {
                "apply CPU microcode/kernel mitigations or set M80_SKIP_CHECK_VULNERABILITIES=1 only after accepting the side-channel risk"
            }
            Self::InvalidKernelKind { .. } => {
                "set M80_KERNEL_KIND to either `stock` or `stripped`"
            }
            Self::CgroupV2Unavailable => {
                "boot the host with a unified cgroup v2 hierarchy or set cgroup_mode = \"disabled\" only for development"
            }
            Self::VsockUnavailable => {
                "load vhost_vsock with `sudo modprobe vhost_vsock` or ensure /dev/vhost-vsock exists"
            }
            Self::TunUnavailable => {
                "load tun with `sudo modprobe tun` or ensure /dev/net/tun exists"
            }
            Self::NfConntrackUnavailable => {
                "load nf_conntrack with `sudo modprobe nf_conntrack` before enabling outbound NAT"
            }
            Self::BridgeNetfilterUnavailable => {
                "load br_netfilter with `sudo modprobe br_netfilter` before enabling outbound NAT"
            }
            Self::BridgeNfCallIptablesDisabled { .. } => {
                "set net.bridge.bridge-nf-call-iptables=1 with sysctl before enabling outbound NAT"
            }
            Self::NfConntrackCapacityTooLow { .. } => {
                "raise net.netfilter.nf_conntrack_max with sysctl or lower M80_MAX_CONCURRENT_VMS"
            }
            Self::InvalidNfConntrackMax { .. } => {
                "inspect /proc/sys/net/netfilter/nf_conntrack_max; it must contain a decimal integer"
            }
            Self::InvalidExpectedConcurrentVms { .. } => {
                "set M80_MAX_CONCURRENT_VMS to a positive decimal u32"
            }
            Self::KernelModulesMissing { .. } => {
                "load the missing modules with `sudo modprobe <name>` or add them to /etc/modules to persist across reboots"
            }
            Self::PrivilegeUnavailable { .. } => {
                "run as root, use `setcap cap_net_admin,cap_sys_admin,cap_mknod,cap_chown,cap_fowner,cap_kill,cap_setuid,cap_setgid,cap_setpcap+ep <binary>`, or set securityContext.capabilities.add in your pod spec"
            }
            Self::FirecrackerBinaryNotFound => {
                "install firecracker to /opt/firecracker/bin/firecracker or set M80_FIRECRACKER_BIN to the binary path"
            }
            Self::FirecrackerVersionMismatch { .. } => {
                "install the expected version or set M80_FIRECRACKER_VERSION to the installed version to skip the version pin"
            }
            Self::FirecrackerVersionCommandFailed { .. } => {
                "run the firecracker binary manually with --version and inspect stderr"
            }
            Self::FirecrackerCveFloorViolation { .. } => {
                "upgrade firecracker to a version fixed for every advisory tracked by m80-preflight"
            }
            Self::FirecrackerSeccompFilterNotFound { .. } => {
                "install the Firecracker advanced seccomp filter bitcode or set M80_FIRECRACKER_SECCOMP_FILTER to its absolute path"
            }
            Self::FirecrackerSeccompFilterEmpty { .. } => {
                "replace the Firecracker advanced seccomp filter with a non-empty compiled bitcode filter"
            }
            Self::JailerBinaryNotFound => {
                "install jailer to /opt/firecracker/bin/jailer (it ships alongside firecracker) or set M80_JAILER_BIN to the binary path"
            }
            Self::JailerHardenBinaryNotFound => {
                "install m80-jailer-harden to /opt/m80/bin/m80-jailer-harden or set M80_JAILER_HARDEN_BIN to the binary path"
            }
            Self::NetHelperBinaryNotFound => {
                "install m80-net-helper to /opt/m80/bin/m80-net-helper or set M80_NET_HELPER_BIN to the binary path"
            }
            Self::HostBinaryManifest(_) => {
                "install /opt/m80/artifacts/host-binaries.manifest.json from the deploy step"
            }
            Self::HostBinaryMissing { .. } => {
                "regenerate host-binaries.manifest.json so it covers every required TCB binary"
            }
            Self::HostBinaryDuplicate { .. } => {
                "remove duplicate entries from host-binaries.manifest.json and reinstall it"
            }
            Self::HostBinaryPathMismatch { .. } => {
                "make the configured binary path match host-binaries.manifest.json or regenerate the manifest after reinstalling binaries"
            }
            Self::BinaryHashMismatch { .. } => {
                "reinstall the host binary from a trusted release bundle and regenerate host-binaries.manifest.json"
            }
            Self::HostBinaryPermission { .. } => {
                "install host binaries as root:root with mode no broader than 0755 and no group/world write bits"
            }
            Self::NonAbsolutePath { .. } => {
                "set the corresponding M80_* path env var to an absolute host path"
            }
            Self::KernelNotFound => {
                "set M80_KERNEL_IMAGE to an absolute path, or place a vmlinux-* file under M80_ARTIFACT_DIR (default /opt/m80/artifacts)"
            }
            Self::RootfsNotFound => {
                "set M80_ROOTFS_IMAGE to the absolute path of a built m80 rootfs image"
            }
            Self::ArtifactDirectoryWritable { .. } => {
                "remove group/world write permission from the artifact directory before running m80"
            }
            Self::ArtifactFileWritable { .. } => {
                "remove group/world write permission from the artifact file before running m80"
            }
            Self::Manifest(_) => {
                "rebuild the guest image with `m80-image-build` to regenerate a valid manifest"
            }
            Self::BuildReceipt(_)
            | Self::BuildReceiptPathMismatch { .. }
            | Self::BuildReceiptManifestMismatch { .. }
            | Self::BuildReceiptArtifactMissing { .. }
            | Self::BuildReceiptArtifactDuplicate { .. }
            | Self::BuildReceiptArtifactPathMismatch { .. }
            | Self::BuildReceiptArtifactHashMismatch { .. } => {
                "rebuild or reinstall the guest artifacts and deploy the matching build receipt"
            }
            Self::RunRootUnavailable { .. } => {
                "create the directory (`sudo mkdir -p /var/run/m80`) and ensure at least 100 MiB of free space is available, or set M80_RUN_ROOT to a different path"
            }
            Self::StorageHelperMissing(_) => {
                "install e2fsprogs (`sudo apt-get install e2fsprogs` on Debian / Ubuntu)"
            }
            Self::CapabilityRead(_) => {
                "the capability subsystem reported an error; check that /proc/*/status is readable and the kernel supports POSIX capabilities"
            }
            Self::PathIo { .. } => "check file permissions and whether the path exists",
            Self::SystemIo { .. } => "inspect the host syscall or command failure above",
        }
    }
}
