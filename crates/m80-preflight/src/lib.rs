//! Host capability checks and binary discovery before VM launch.
//!
//! See `README.md` for the black-box contract.
//! Behavior captures: bead epic `m80-a7g` (`br show m80-a7g`).

#![deny(missing_docs)]

use std::io;
use std::path::PathBuf;

use caps::Capability;
use caps::CapsHashSet;
use serde::{Deserialize, Serialize};

use m80_image_manifest::{Manifest, ManifestError};

mod artifacts;
mod binary;
mod checks;
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
    /// Resolved jailer binary path.
    pub jailer_bin: PathBuf,
    /// Resolved kernel image path.
    pub kernel: PathBuf,
    /// Resolved rootfs image path.
    pub rootfs: PathBuf,
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
    pub fn render_table(&self) -> String {
        table::render(&self.report)
    }
}

pub use artifacts::{
    verify_artifacts, ArtifactPreflight, ArtifactPreflightConfig, DEFAULT_ARTIFACT_DIR,
    DEFAULT_RUN_ROOT, ENV_ARTIFACT_DIR, ENV_KERNEL_IMAGE, ENV_KERNEL_KIND, ENV_ROOTFS_IMAGE,
    ENV_RUN_ROOT, MIN_RUN_ROOT_FREE_BYTES, REQUIRED_STORAGE_HELPERS,
};
pub use binary::{
    discover_binaries, BinaryDiscovery, BinaryDiscoveryConfig, DEFAULT_FIRECRACKER_BIN,
    DEFAULT_JAILER_BIN, ENV_FIRECRACKER_BIN, ENV_FIRECRACKER_VERSION, ENV_JAILER_BIN,
};
pub use checks::{run, run_with_configs};

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

    /// `jailer` binary not found.
    #[error("jailer binary not found")]
    JailerBinaryNotFound,

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

    /// Manifest read/validate failed.
    #[error("manifest: {0}")]
    Manifest(#[from] ManifestError),

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

    /// Underlying I/O failure.
    #[error("i/o: {0}")]
    Io(#[from] io::Error),
}

impl PreflightError {
    /// Actionable operator hint for this error.
    ///
    /// Rendered by the CLI in non-JSON mode as a `hint:` line after the error
    /// message; omitted from the JSON `detail` field so machine readers see a
    /// clean error string.
    pub fn hint(&self) -> &'static str {
        match self {
            Self::UnsupportedHostPlatform { .. } => {
                "m80 requires a Linux host; macOS and Windows are not supported"
            }
            Self::KvmUnavailable { .. } => {
                "ensure KVM is enabled in the host kernel and /dev/kvm exists"
            }
            Self::KvmNotWritable { .. } => {
                "add your user to the `kvm` group (`sudo usermod -aG kvm $USER`) or run m80 as root"
            }
            Self::KernelModulesMissing { .. } => {
                "load the missing modules with `sudo modprobe <name>` or add them to /etc/modules to persist across reboots"
            }
            Self::PrivilegeUnavailable { .. } => {
                "run as root, use `setcap cap_net_admin,cap_sys_admin,cap_mknod,cap_chown,cap_fowner,cap_kill+ep <binary>`, or set securityContext.capabilities.add in your pod spec"
            }
            Self::FirecrackerBinaryNotFound => {
                "install firecracker to /opt/firecracker/bin/firecracker or set M80_FIRECRACKER_BIN to the binary path"
            }
            Self::FirecrackerVersionMismatch { .. } => {
                "install the expected version or set M80_FIRECRACKER_VERSION to the installed version to skip the version pin"
            }
            Self::JailerBinaryNotFound => {
                "install jailer to /opt/firecracker/bin/jailer (it ships alongside firecracker) or set M80_JAILER_BIN to the binary path"
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
            Self::Manifest(_) => {
                "rebuild the guest image with `m80-image-build` to regenerate a valid manifest"
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
            Self::Io(_) => "check file permissions and whether the path exists",
        }
    }
}
