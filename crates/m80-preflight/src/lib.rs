//! Host capability checks and binary discovery before VM launch.
//!
//! See `README.md` for the black-box contract.
//! Behavior captures: bead epic `m80-a7g` (`br show m80-a7g`).
//!
//! # Type-pinning pass
//!
//! Public surface is declared here; bodies are `todo!()`. Implementation lands
//! in a later wave.

#![deny(missing_docs)]

use std::io;
use std::path::PathBuf;

use caps::Capability;
use serde::{Deserialize, Serialize};

use m80_image_manifest::{Manifest, ManifestError};

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
        todo!()
    }
}

/// Run the preflight checklist. Fail-closed on any single failure.
pub fn run() -> Result<Discovery, PreflightError> {
    todo!()
}

/// Errors surfaced by preflight. Each variant carries actionable hint text
/// when rendered.
#[derive(Debug, thiserror::Error)]
pub enum PreflightError {
    /// Host kernel is not Linux.
    #[error("unsupported host platform: {0}")]
    UnsupportedHostPlatform(String),
    /// `/dev/kvm` is missing or not writable.
    #[error("/dev/kvm unavailable; add user to the kvm group or run with sufficient privilege")]
    KvmUnavailable,
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
        /// Pinned version (from manifest or env).
        expected: String,
        /// Reported version.
        actual: String,
    },
    /// `jailer` binary not found.
    #[error("jailer binary not found")]
    JailerBinaryNotFound,
    /// No `vmlinux-*` discovered.
    #[error("kernel image not found")]
    KernelNotFound,
    /// No rootfs image discovered (or env override missing).
    #[error("rootfs image not found")]
    RootfsNotFound,
    /// Manifest read/validate failed.
    #[error("manifest: {0}")]
    Manifest(#[from] ManifestError),
    /// Run-root has insufficient capacity.
    #[error("run-root has insufficient capacity")]
    InsufficientRunRootCapacity,
    /// One of the storage helpers (`mkfs.ext4`, `debugfs`, `e2fsck`) is missing.
    #[error("storage helper missing on PATH: {0}")]
    StorageHelperMissing(String),
    /// Underlying I/O failure.
    #[error("i/o: {0}")]
    Io(#[from] io::Error),
}
