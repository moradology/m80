//! Host capability checks and binary discovery before VM launch.
//!
//! See `README.md` for the black-box contract.
//! Behavior captures: bead epic `m80-a7g` (`br show m80-a7g`).

#![deny(missing_docs)]

use std::io;
use std::path::PathBuf;

use caps::Capability;
use serde::{Deserialize, Serialize};

use m80_image_manifest::{Manifest, ManifestError};

mod checks;
mod table;

/// Linux capabilities m80 needs in its effective set when not running as
/// `euid == 0`. A process holding all of these (e.g., a binary with
/// `setcap cap_net_admin,cap_sys_admin,cap_mknod,cap_chown,cap_fowner,cap_kill+ep`,
/// or a container with the same `securityContext.capabilities.add`) passes
/// the privilege precondition without being root.
pub const REQUIRED_CAPABILITIES: &[Capability] = &[
    Capability::CAP_NET_ADMIN,
    Capability::CAP_SYS_ADMIN,
    Capability::CAP_MKNOD,
    Capability::CAP_CHOWN,
    Capability::CAP_FOWNER,
    Capability::CAP_KILL,
];

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

/// Run the preflight checklist. Fail-closed on any single failure.
pub fn run() -> Result<Discovery, PreflightError> {
    checks::run_all()
}

/// Errors surfaced by preflight. Each variant carries actionable hint text
/// when rendered.
#[derive(Debug, thiserror::Error)]
pub enum PreflightError {
    /// Host kernel is not Linux.
    #[error(
        "unsupported host platform: {0}\n\
         hint: m80 requires a Linux host; macOS and Windows are not supported"
    )]
    UnsupportedHostPlatform(String),

    /// `/dev/kvm` is missing or not writable.
    #[error(
        "/dev/kvm unavailable\n\
         hint: ensure KVM is enabled in the host kernel and add your user to \
         the `kvm` group (`sudo usermod -aG kvm $USER`) or run m80 as root"
    )]
    KvmUnavailable,

    /// Required kernel modules are not loaded/loadable.
    #[error(
        "kernel modules missing: {missing:?}\n\
         hint: load the missing modules with `sudo modprobe <name>` or add \
         them to /etc/modules to persist across reboots"
    )]
    KernelModulesMissing {
        /// Names of the missing modules (e.g., `tap`, `bridge`).
        missing: Vec<String>,
    },

    /// `geteuid() != 0` AND one or more capabilities in
    /// [`REQUIRED_CAPABILITIES`] are absent from the effective set.
    /// Operator must run as root, `setcap` the binary, or grant the caps via
    /// a container `securityContext`.
    #[error(
        "insufficient privilege; missing capabilities: {missing_caps:?}\n\
         hint: run as root, use `setcap cap_net_admin,cap_sys_admin,\
         cap_mknod,cap_chown,cap_fowner,cap_kill+ep <binary>`, \
         or set securityContext.capabilities.add in your pod spec"
    )]
    PrivilegeUnavailable {
        /// Capabilities that were required but not present.
        missing_caps: Vec<Capability>,
    },

    /// `firecracker` not found via env or default path.
    #[error(
        "firecracker binary not found\n\
         hint: install firecracker to /opt/firecracker/bin/firecracker or \
         set M80_FIRECRACKER_BIN to the binary path"
    )]
    FirecrackerBinaryNotFound,

    /// `firecracker --version` did not match the configured pin.
    #[error(
        "firecracker version mismatch: expected {expected}, got {actual}\n\
         hint: install the expected version or set M80_FIRECRACKER_VERSION \
         to the installed version to skip the version pin"
    )]
    FirecrackerVersionMismatch {
        /// Pinned version (from env).
        expected: String,
        /// Reported version.
        actual: String,
    },

    /// `jailer` binary not found.
    #[error(
        "jailer binary not found\n\
         hint: install jailer to /opt/firecracker/bin/jailer (it ships \
         alongside firecracker) or set M80_JAILER_BIN to the binary path"
    )]
    JailerBinaryNotFound,

    /// No `vmlinux-*` discovered.
    #[error(
        "kernel image not found\n\
         hint: set M80_KERNEL_IMAGE to an absolute path, or place a \
         vmlinux-* file under M80_ARTIFACT_DIR \
         (default /opt/m80/artifacts)"
    )]
    KernelNotFound,

    /// No rootfs image discovered (or env override missing).
    #[error(
        "rootfs image not found\n\
         hint: set M80_ROOTFS_IMAGE to the absolute path of a built \
         m80 rootfs image"
    )]
    RootfsNotFound,

    /// Manifest read/validate failed.
    #[error(
        "manifest: {0}\n\
         hint: rebuild the guest image with `m80-image-build` to regenerate \
         a valid manifest"
    )]
    Manifest(#[from] ManifestError),

    /// Run-root directory is absent or has insufficient free capacity
    /// (< 100 MiB). This variant covers both "directory does not exist" and
    /// "not enough space" so a single variant name handles both conditions.
    #[error(
        "run-root unavailable: {reason}\n\
         hint: create the directory (`sudo mkdir -p /var/run/m80`) and \
         ensure at least 100 MiB of free space is available, or set \
         M80_RUN_ROOT to a different path"
    )]
    RunRootUnavailable {
        /// Human-readable reason (missing directory or insufficient space).
        reason: String,
    },

    /// One of the storage helpers (`mkfs.ext4`, `debugfs`, `e2fsck`) is missing.
    #[error(
        "storage helper missing on PATH: {0}\n\
         hint: install e2fsprogs (`sudo apt-get install e2fsprogs` on Debian \
         / Ubuntu)"
    )]
    StorageHelperMissing(String),

    /// Underlying I/O failure.
    #[error(
        "i/o: {0}\n\
         hint: check file permissions and whether the path exists"
    )]
    Io(#[from] io::Error),
}
