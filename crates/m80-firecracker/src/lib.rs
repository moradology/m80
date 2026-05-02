//! VM lifecycle state machine: composes the m80 foundation crates into a
//! launchable sandbox. The fat consumer; itself thin in business logic.
//!
//! See `README.md` for the black-box contract.
//! Behavior captures: bead epics `m80-t01` (lifecycle), `m80-19i`
//! (concurrency/admission), `m80-ynh` (cleanup/drain), `m80-4ef` (errors),
//! `m80-v7t` (configuration).
//!
//! # Type-pinning pass
//!
//! Public surface is declared here; bodies are `todo!()`. Implementation lands
//! in a later wave.

#![deny(missing_docs)]

use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use m80_firecracker_client::ClientError;
use m80_image_manifest::ManifestError;
use m80_jailer::JailerError;
use m80_net_outbound::NetError;
use m80_preflight::PreflightError;
use m80_storage::{ChangeSet, StorageError};
use m80_vsock::VsockError;

pub use m80_net_mode::NetworkPolicy;
// The wire-shaped exec types are owned by `m80-proto` (the only crate the
// host and the in-VM `m80-guestd` daemon both depend on). Re-export them
// here so consumers of the orchestrator don't need to also depend on
// `m80-proto` for the request/response shape.
pub use m80_proto::{ExecRequest, ExecResponse, ExecStatus, ExecTiming};

// =====================================================================
// Backend (long-lived, per-process)
// =====================================================================

/// Long-lived backend handle a service holds. Wraps the admission semaphore
/// and the discovery info from preflight. One per process.
#[derive(Debug)]
pub struct Backend {
    _config: BackendConfig,
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

impl Backend {
    /// Construct a backend handle. Run preflight first and pass the
    /// `Discovery` into the [`BackendConfig`].
    pub fn new(_config: BackendConfig) -> Result<Self, FcError> {
        todo!()
    }

    /// Acquire one admission permit + create a [`Sandbox`] in Created state.
    pub fn admit(&self, _config: SandboxConfig) -> Result<Sandbox, FcError> {
        todo!()
    }

    /// Reveal the merged effective config for diagnostics.
    pub fn show_effective_config(&self) -> EffectiveConfig {
        todo!()
    }

    /// Run the run-root recovery scan now (also runs in the background).
    pub fn recover_stale_run_root(&self) -> Result<(), FcError> {
        todo!()
    }
}

// =====================================================================
// Sandbox lifecycle: Created → Running → Stopped → Deleted
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
    /// Number of vCPUs (default: from manifest first-line VM sizing).
    pub vcpu_count: Option<u32>,
    /// Memory in MiB (default: from manifest first-line VM sizing).
    pub mem_size_mib: Option<u32>,
    /// Boot args appended to the kernel command line.
    pub boot_args: Option<String>,
    /// Optional default exec timeout.
    pub default_exec_timeout: Option<Duration>,
}

/// A sandbox in `Created` state — admission permit held, no I/O performed yet.
#[derive(Debug)]
pub struct Sandbox {
    _config: SandboxConfig,
}

impl Sandbox {
    /// Standalone constructor for callers without a [`Backend`]. The admission
    /// semaphore is bypassed; the caller is responsible for concurrency.
    pub fn new(_config: SandboxConfig) -> Result<Sandbox, FcError> {
        todo!()
    }

    /// Run the strict preboot pipeline:
    /// preflight → run-root prep → lease acquisition → storage prep →
    /// jailer materialize → cgroup subtree → network realize →
    /// guest config injection → vsock → REST PUTs → InstanceStart →
    /// ready probe.
    pub fn launch(self) -> Result<RunningSandbox, FcError> {
        todo!()
    }
}

/// A sandbox in `Running` state — VM booted, vsock ready, ready to accept
/// exec requests.
#[derive(Debug)]
pub struct RunningSandbox {
    _vm_id: String,
}

impl RunningSandbox {
    /// VM id assigned to this sandbox.
    pub fn vm_id(&self) -> &str {
        todo!()
    }

    /// Send one exec request to the in-VM daemon. One outstanding exec per
    /// sandbox in v0.1.
    pub fn exec(&mut self, _req: ExecRequest) -> Result<ExecResponse, FcError> {
        todo!()
    }

    /// Four-phase teardown:
    /// admission_fence → bounded_stop → optional change-extract → release.
    /// On `x86_64` the bounded_stop is graceful (`SendCtrlAltDel`);
    /// elsewhere it's forced.
    pub fn stop(self) -> Result<StoppedSandbox, FcError> {
        todo!()
    }

    /// Last-resort: SIGKILL the firecracker + jailer pids and preserve the
    /// run-dir for triage.
    pub fn force_kill(self) -> Result<StoppedSandbox, FcError> {
        todo!()
    }
}

/// A sandbox in `Stopped` state — VM exited, scratch image quiesced,
/// run-dir intact.
#[derive(Debug)]
pub struct StoppedSandbox {
    _run_dir: PathBuf,
}

impl StoppedSandbox {
    /// Run-dir for this stopped sandbox.
    pub fn run_dir(&self) -> &Path {
        todo!()
    }

    /// Opt-in change extraction: e2fsck → debugfs → admissibility scan →
    /// staging tree → atomic swap.
    pub fn extract_changes(&self, _into: &Path) -> Result<ChangeSet, FcError> {
        todo!()
    }

    /// Remove the run-dir and release ownership. Drops the admission permit.
    pub fn delete(self) -> Result<(), FcError> {
        todo!()
    }

    /// Keep the run-dir for offline triage; release the admission permit but
    /// leave the on-disk residue. Returns the preserved run-dir path.
    pub fn preserve_for_triage(self) -> Result<PathBuf, FcError> {
        todo!()
    }
}

// =====================================================================
// Error sum
// =====================================================================

/// Top-level error for `m80-firecracker`. Each variant tells the caller
/// which phase failed; the inner cause carries phase-specific detail.
#[derive(Debug, thiserror::Error)]
pub enum FcError {
    /// Preflight check failed.
    #[error("preflight: {0}")]
    Preflight(#[from] PreflightError),
    /// Manifest read/validate failed.
    #[error("manifest: {0}")]
    Manifest(#[from] ManifestError),
    /// Storage operation failed.
    #[error("storage: {0}")]
    Storage(#[from] StorageError),
    /// Jailer materialization or recovery failed.
    #[error("jailer: {0}")]
    Jailer(#[from] JailerError),
    /// Network realization or cleanup failed.
    #[error("network: {0}")]
    Network(#[from] NetError),
    /// Firecracker REST API call failed.
    #[error("client: {0}")]
    Client(#[from] ClientError),
    /// Vsock channel operation failed. Wire-protocol errors arrive here as
    /// `VsockError::Proto(...)` since vsock is the only transport that
    /// speaks `m80-proto` envelopes in this crate; there is no separate
    /// `Proto` variant.
    #[error("vsock: {0}")]
    Vsock(#[from] VsockError),
    /// Admission was refused (semaphore at limit; no permit available).
    #[error("admission refused: {limit} concurrent VMs already running")]
    AdmissionRefused {
        /// Configured admission limit.
        limit: u32,
    },
    /// The lifecycle state machine was in an unexpected state.
    #[error("invalid lifecycle state: expected {expected}, got {actual}")]
    InvalidState {
        /// State the operation expected.
        expected: &'static str,
        /// State the sandbox was actually in.
        actual: &'static str,
    },
    /// Underlying I/O failure.
    #[error("i/o: {0}")]
    Io(#[from] io::Error),
    /// Configuration loading or merging failure.
    #[error("config: {0}")]
    Config(String),
}
