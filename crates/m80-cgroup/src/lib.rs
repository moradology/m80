//! Per-VM cgroup v2 subtree: create, enforce CPU/memory/pids, clean up.
//!
//! See `README.md` for the black-box contract.
//! Behavior captures: bead epic `m80-84x` (`br show m80-84x`).
//!
//! # Type-pinning pass
//!
//! Public surface is declared here; bodies are `todo!()`. Implementation lands
//! in a later wave.

#![deny(missing_docs)]

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use m80_jailer::MaterializedJail;

/// One per-VM cgroup v2 subtree under `/sys/fs/cgroup/m80-firecracker/<vm-id>`.
#[derive(Debug)]
pub struct Subtree {
    _path: PathBuf,
}

impl Subtree {
    /// Probe whether this host supports unified cgroup v2 mode. Run this
    /// before [`Subtree::create`] from preflight.
    pub fn probe() -> Result<(), CgroupError> {
        todo!()
    }

    /// Create the per-VM subtree, enable cpu/memory/pids controllers, assign
    /// both `jail.jailer_pid` and `jail.firecracker_pid` to the subtree.
    pub fn create(_vm_id: &str, _jail: &MaterializedJail) -> Result<Self, CgroupError> {
        todo!()
    }

    /// Apply per-controller limits. Fields set to `None` leave the existing
    /// value alone.
    pub fn apply_limits(&self, _limits: &Limits) -> Result<(), CgroupError> {
        todo!()
    }

    /// Path of the materialized cgroup subtree. Persisted at
    /// `<run_dir>/cgroup-path.txt` for offline triage.
    pub fn path(&self) -> &Path {
        todo!()
    }
}

/// Per-controller limits for one subtree.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Limits {
    /// `cpu.max`. None = leave existing.
    pub cpu_max: Option<CpuMax>,
    /// `memory.max` in bytes. None = leave existing.
    pub memory_max: Option<u64>,
    /// `pids.max`. None = leave existing.
    pub pids_max: Option<u32>,
}

/// `cpu.max` value: either a concrete `(quota, period)` pair or `Max`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CpuMax {
    /// Concrete cpu quota and period in microseconds.
    Quota {
        /// Allowed runtime per period, in microseconds.
        quota_us: u64,
        /// Period over which the quota applies, in microseconds.
        period_us: u64,
    },
    /// No quota (the default).
    Max,
}

/// Best-effort cleanup of a stale subtree from a prior crashed run.
pub fn cleanup_orphan_subtree(_vm_id: &str) -> Result<(), CgroupError> {
    todo!()
}

/// Errors surfaced by cgroup operations.
#[derive(Debug, thiserror::Error)]
pub enum CgroupError {
    /// Cgroup mode was requested without a materialized jailer.
    #[error("cgroup mode requires a materialized jailer")]
    RequiresJailer,
    /// Host is not in unified cgroup v2 mode.
    #[error("host is not in unified cgroup v2 mode")]
    UnsupportedHostMode,
    /// A required cgroup v2 controller is not enabled in the parent.
    #[error("controller not enabled: {0}")]
    ControllerNotEnabled(String),
    /// Underlying I/O failure.
    #[error("i/o: {0}")]
    Io(#[from] io::Error),
}
