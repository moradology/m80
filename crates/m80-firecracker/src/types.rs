//! Public struct and enum definitions for `m80-firecracker`.
//! Implementation blocks live in the module that owns the type's domain.

use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use m80_firecracker_client::Client;
use m80_jailer::{JailedFirecracker, MaterializedJail};
use m80_storage::{Rootfs, Scratch};
use m80_vsock::Channel;

pub use m80_net_mode::NetworkPolicy;

// =====================================================================
// Admission semaphore
// =====================================================================

/// Inner state of the admission semaphore: `(available_permits, Condvar)`.
pub type SemaphoreInner = (Mutex<u32>, Condvar);

/// Shared admission semaphore.
pub type Semaphore = Arc<SemaphoreInner>;

/// An acquired admission permit. Releasing it (via Drop) returns one slot to
/// the semaphore.
pub struct AdmissionPermit {
    /// Handle back to the semaphore so Drop can return the slot.
    pub sem: Semaphore,
    /// The configured maximum (for error messages on exhaustion).
    pub limit: u32,
}

impl std::fmt::Debug for AdmissionPermit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AdmissionPermit")
            .field("limit", &self.limit)
            .finish()
    }
}

impl Drop for AdmissionPermit {
    fn drop(&mut self) {
        let (lock, cvar) = self.sem.as_ref();
        let mut available = lock.lock().unwrap_or_else(|p| p.into_inner());
        *available += 1;
        cvar.notify_one();
    }
}

// =====================================================================
// Backend
// =====================================================================

/// Long-lived backend handle a service holds. Wraps the admission semaphore
/// and the discovery info from preflight. One per process.
pub struct Backend {
    /// Merged + annotated configuration.
    pub config: BackendConfig,
    /// Effective configuration snapshot (for `show_effective_config`).
    pub effective: EffectiveConfig,
    /// Admission semaphore: `(available_permits, Condvar)`.
    pub semaphore: Semaphore,
}

impl std::fmt::Debug for Backend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Backend")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

/// Configuration for [`Backend`]. Built by merging defaults, file config,
/// env vars, and CLI flags in that precedence order.
#[derive(Debug, Clone)]
pub struct BackendConfig {
    /// Discovery output from `m80-preflight::run()`.
    pub discovery: m80_preflight::Discovery,
    /// Maximum concurrent VMs admitted on this host.
    pub max_concurrent_vms: u32,
    /// Per-process run-root directory.
    pub run_root: PathBuf,
    /// UID for the jailed firecracker process.
    pub jail_uid: u32,
    /// GID for the jailed firecracker process.
    pub jail_gid: u32,
    /// Cgroup mode (unified-v2 or disabled).
    pub cgroup_mode: CgroupMode,
}

/// Cgroup hardening mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CgroupMode {
    /// Cgroup v2 unified hierarchy enforcement.
    UnifiedV2,
    /// No cgroup hardening.
    Disabled,
}

/// Snapshot of the merged configuration with each field tagged by source.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectiveConfig {
    /// One row per resolved field.
    pub fields: Vec<EffectiveField>,
}

/// One row in [`EffectiveConfig`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectiveField {
    /// Field name (e.g., `"max_concurrent_vms"`).
    pub name: String,
    /// Effective value rendered as a string.
    pub value: String,
    /// Source that supplied the value.
    pub source: ConfigSource,
}

/// Where one [`EffectiveField`] value came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigSource {
    /// Built-in default.
    Default,
    /// `/etc/m80/config.toml`.
    SystemFile,
    /// `~/.config/m80/config.toml`.
    UserFile,
    /// `M80_*` environment variable.
    Env,
    /// Command-line flag.
    Flag,
}

// =====================================================================
// Sandbox (Created state)
// =====================================================================

/// Per-VM configuration. Built once per [`Sandbox`].
#[derive(Debug, Clone)]
pub struct SandboxConfig {
    /// Optional caller-provided VM id; auto-derived when absent.
    pub vm_id: Option<String>,
    /// Optional host directory hydrated into a scratch ext4 inside the VM.
    pub workspace: Option<PathBuf>,
    /// Caller intent for network egress.
    pub network: NetworkPolicy,
    /// Number of vCPUs (default: 1).
    pub vcpu_count: Option<u32>,
    /// Memory in MiB (default: 1024).
    pub mem_size_mib: Option<u32>,
    /// Boot args appended to the kernel command line.
    pub boot_args: Option<String>,
    /// Optional default exec timeout.
    pub default_exec_timeout: Option<Duration>,
}

/// A sandbox in `Created` state — admission permit held, no I/O performed yet.
pub struct Sandbox {
    /// Per-VM configuration.
    pub config: SandboxConfig,
    /// Admission permit; dropped if launch fails.
    pub permit: AdmissionPermit,
    /// Reference to the backend that created this sandbox.
    pub backend: Arc<Backend>,
}

impl std::fmt::Debug for Sandbox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Sandbox")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

// =====================================================================
// RunningSandbox (Running state)
// =====================================================================

/// A sandbox in `Running` state — VM booted, vsock ready, ready to accept
/// exec requests.
pub struct RunningSandbox {
    /// VM identifier.
    pub vm_id: String,
    /// Per-VM run directory.
    pub run_dir: PathBuf,
    /// The materialized jailer chroot.
    pub jail: MaterializedJail,
    /// Cgroup subtree (Some if UnifiedV2 mode).
    pub cgroup: Option<m80_cgroup::Subtree>,
    /// Open vsock channel to the in-VM guestd.
    pub channel: Channel,
    /// Per-VM rootfs clone.
    pub rootfs: Rootfs,
    /// Scratch image (Some if workspace is configured).
    pub scratch: Option<Scratch>,
    /// REST client to the Firecracker process.
    pub client: Client,
    /// Live firecracker + jailer pids.
    pub firecracker: JailedFirecracker,
    /// Admission permit; held for the lifetime of this sandbox.
    pub permit: AdmissionPermit,
    /// Reference to the backend.
    pub backend: Arc<Backend>,
}

impl std::fmt::Debug for RunningSandbox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RunningSandbox")
            .field("vm_id", &self.vm_id)
            .field("run_dir", &self.run_dir)
            .finish_non_exhaustive()
    }
}

// =====================================================================
// StoppedSandbox (Stopped state)
// =====================================================================

/// A sandbox in `Stopped` state — VM exited, scratch image quiesced,
/// run-dir intact.
pub struct StoppedSandbox {
    /// VM identifier.
    pub vm_id: String,
    /// Per-VM run directory.
    pub run_dir: PathBuf,
    /// Scratch image (for `extract_changes`; consumed when extracted).
    pub scratch: Option<Scratch>,
    /// Admission permit; dropped on `delete()` or `preserve_for_triage()`.
    pub permit: AdmissionPermit,
    /// Run-root path (needed for `preserve_for_triage`).
    pub run_root: PathBuf,
}

impl std::fmt::Debug for StoppedSandbox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StoppedSandbox")
            .field("vm_id", &self.vm_id)
            .field("run_dir", &self.run_dir)
            .finish_non_exhaustive()
    }
}

// =====================================================================
// StoragePrep (internal to launch)
// =====================================================================

/// Bundle of per-VM storage resources produced by the storage-prep phase.
pub struct StoragePrep {
    /// The per-VM rootfs clone.
    pub rootfs: Rootfs,
    /// The scratch image (Some if a workspace was requested).
    pub scratch: Option<Scratch>,
}

impl std::fmt::Debug for StoragePrep {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StoragePrep")
            .field("has_scratch", &self.scratch.is_some())
            .finish()
    }
}

// =====================================================================
// RealizedNetwork (internal to launch)
// =====================================================================

/// Resolved network state after phase 6.
#[derive(Debug)]
pub enum RealizedNetwork {
    /// No NIC configured; iptables untouched.
    NoEgress,
}
