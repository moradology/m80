//! Per-VM cgroup v2 subtree: create, enforce CPU/memory/pids, clean up.
//!
//! See `README.md` for the black-box contract.
//! Behavior captures: bead epic `m80-84x` (`br show m80-84x`).

#![deny(missing_docs)]

use std::fs;
use std::io::{self, Write as IoWrite};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tracing::warn;

use m80_jailer::{JailedFirecracker, MaterializedJail};

/// Root cgroup directory for all m80 VMs.
const CGROUP_ROOT: &str = "/sys/fs/cgroup/m80-firecracker";

/// Cgroup v2 global root.
const CGROUP_V2_ROOT: &str = "/sys/fs/cgroup";

const REQUIRED_SUBTREE_CONTROL: &str = "+cpu +memory +pids\n";
const DEFAULT_CPU_QUOTA_US: u64 = 100_000;
const DEFAULT_CPU_PERIOD_US: u64 = 100_000;
const DEFAULT_MEMORY_MAX_BYTES: u64 = 1_610_612_736;
const DEFAULT_PIDS_MAX: u32 = 128;

/// One per-VM cgroup v2 subtree under `/sys/fs/cgroup/m80-firecracker/<vm-id>`.
#[derive(Debug)]
pub struct Subtree {
    path: PathBuf,
}

impl Subtree {
    /// Probe whether this host has unified cgroup v2 — call from preflight
    /// before any [`Subtree::create`].
    pub fn probe() -> Result<(), CgroupError> {
        let mounts = fs::read_to_string("/proc/mounts").map_err(|source| CgroupError::Io {
            path: PathBuf::from("/proc/mounts"),
            source,
        })?;
        probe_mounts(&mounts)
    }

    /// Create the per-VM subtree under `m80-firecracker/<vm_id>/`, enable
    /// cpu/memory/pids controllers in the parent, and enroll
    /// `jailed.firecracker_pid`. `jailer_pid` is intentionally omitted —
    /// after `m80-jailer` the two pids are equal (jailer execs into
    /// firecracker), so writing it would be redundant.
    pub fn create(
        vm_id: &str,
        jail: &MaterializedJail,
        jailed: &JailedFirecracker,
    ) -> Result<Self, CgroupError> {
        let parent = PathBuf::from(CGROUP_ROOT);

        // create_dir_all is race-safe against concurrent sandboxes.
        fs::create_dir_all(&parent).map_err(|source| CgroupError::Io {
            path: parent.clone(),
            source,
        })?;

        let subtree_control = parent.join("cgroup.subtree_control");
        if let Err(cgroup_err) = write_cgroup_file(&subtree_control, REQUIRED_SUBTREE_CONTROL) {
            // Translate to ControllerNotEnabled when we can name the missing
            // one; otherwise propagate the original error unchanged.
            let controllers_path = parent.join("cgroup.controllers");
            if let Ok(controllers) = fs::read_to_string(&controllers_path) {
                for name in ["cpu", "memory", "pids"] {
                    if !controllers.split_whitespace().any(|c| c == name) {
                        return Err(CgroupError::ControllerNotEnabled(name.to_owned()));
                    }
                }
            }
            return Err(cgroup_err);
        }

        let leaf = Self::leaf_path(vm_id);
        fs::create_dir_all(&leaf).map_err(|source| CgroupError::Io {
            path: leaf.clone(),
            source,
        })?;

        let procs = leaf.join("cgroup.procs");
        for pid in pid_assignment_list(jailed) {
            write_cgroup_file(&procs, &format!("{pid}\n"))?;
        }

        let cgroup_path_txt = jail.plan.config.run_dir.join("cgroup-path.txt");
        let leaf_str = format!("{}\n", leaf.display());
        fs::write(&cgroup_path_txt, leaf_str.as_bytes()).map_err(|source| CgroupError::Io {
            path: cgroup_path_txt.clone(),
            source,
        })?;

        Ok(Subtree { path: leaf })
    }

    /// Apply per-controller limits. Fields set to `None` leave the existing
    /// value alone.
    pub fn apply_limits(&self, limits: &Limits) -> Result<(), CgroupError> {
        if let Some(cpu_max) = &limits.cpu_max {
            let val = match cpu_max {
                CpuMax::Quota {
                    quota_us,
                    period_us,
                } => {
                    format!("{quota_us} {period_us}\n")
                }
                CpuMax::Max => "max 100000\n".to_owned(),
            };
            write_cgroup_file(&self.path.join("cpu.max"), &val)?;
        }

        if let Some(mem) = limits.memory_max {
            write_cgroup_file(&self.path.join("memory.max"), &format!("{mem}\n"))?;
        }

        if let Some(pids) = limits.pids_max {
            write_cgroup_file(&self.path.join("pids.max"), &format!("{pids}\n"))?;
        }

        Ok(())
    }

    /// Path of the materialized cgroup subtree. Persisted at
    /// `<run_dir>/cgroup-path.txt` for offline triage.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Compute the absolute cgroup v2 leaf path for `vm_id`.
    pub fn leaf_path(vm_id: &str) -> PathBuf {
        PathBuf::from(CGROUP_ROOT).join(vm_id)
    }
}

impl Drop for Subtree {
    fn drop(&mut self) {
        if let Err(e) = fs::remove_dir(&self.path) {
            warn!("drop: rmdir({}) failed: {e}", self.path.display());
        }
    }
}

/// Best-effort cleanup of a stale subtree from a prior crashed run.
///
/// If the directory does not exist, returns `Ok(())`. If it exists and
/// `cgroup.procs` is non-empty, logs a warning and returns `Ok(())`.
/// If it exists and is empty, removes it.
pub fn cleanup_orphan_subtree(vm_id: &str) -> Result<(), CgroupError> {
    let leaf = Subtree::leaf_path(vm_id);
    if !leaf.exists() {
        return Ok(());
    }

    let procs_path = leaf.join("cgroup.procs");
    let procs = fs::read_to_string(&procs_path).map_err(|source| CgroupError::Io {
        path: procs_path.clone(),
        source,
    })?;

    if !procs.trim().is_empty() {
        warn!(
            "cleanup_orphan_subtree: {} still has live pids, leaving in place: {:?}",
            leaf.display(),
            procs.trim()
        );
        return Ok(());
    }

    fs::remove_dir(&leaf).map_err(|source| CgroupError::Io {
        path: leaf.clone(),
        source,
    })?;

    Ok(())
}

/// Per-controller limits for one subtree.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    /// `cpu.max`. None = leave existing.
    pub cpu_max: Option<CpuMax>,
    /// `memory.max` in bytes. None = leave existing.
    pub memory_max: Option<u64>,
    /// `pids.max`. None = leave existing.
    pub pids_max: Option<u32>,
}

impl Limits {
    /// m80's default VM resource limit profile.
    ///
    /// CPU is one full 100 ms CPU period, memory is 1.5 GiB, and pids are
    /// capped at 128.
    pub fn m80_default() -> Self {
        Self {
            cpu_max: Some(CpuMax::Quota {
                quota_us: DEFAULT_CPU_QUOTA_US,
                period_us: DEFAULT_CPU_PERIOD_US,
            }),
            memory_max: Some(DEFAULT_MEMORY_MAX_BYTES),
            pids_max: Some(DEFAULT_PIDS_MAX),
        }
    }
}

/// `cpu.max` value: either a concrete `(quota, period)` pair or `Max`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields, tag = "kind", rename_all = "snake_case")]
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

/// Errors surfaced by cgroup operations.
#[derive(Debug, thiserror::Error)]
pub enum CgroupError {
    /// Host is not in unified cgroup v2 mode.
    #[error("host is not in unified cgroup v2 mode")]
    UnsupportedHostMode,
    /// A required cgroup v2 controller is not enabled in the parent.
    #[error("controller not enabled: {0}")]
    ControllerNotEnabled(String),
    /// Underlying I/O failure; carries the path so the caller doesn't have
    /// to guess which file failed.
    #[error("i/o on {}: {source}", path.display())]
    Io {
        /// File the I/O was attempted against.
        path: PathBuf,
        /// Underlying I/O error.
        #[source]
        source: io::Error,
    },
}

/// Probe a `/proc/mounts`-shaped string for the unified-v2 hierarchy.
/// Separated from [`Subtree::probe`] so integration tests can pass a
/// synthetic mounts string without touching the host filesystem.
#[doc(hidden)]
pub fn probe_mounts(mounts: &str) -> Result<(), CgroupError> {
    let has_v2 = mounts.lines().any(|line| {
        let mut cols = line.split_whitespace();
        let _dev = cols.next();
        let mount_point = cols.next().unwrap_or("");
        let fs_type = cols.next().unwrap_or("");
        fs_type == "cgroup2" && mount_point == CGROUP_V2_ROOT
    });

    if !has_v2 {
        return Err(CgroupError::UnsupportedHostMode);
    }

    // Also confirm the smoking-gun file is readable.
    let control_file = Path::new(CGROUP_V2_ROOT).join("cgroup.subtree_control");
    if !control_file.exists() {
        return Err(CgroupError::UnsupportedHostMode);
    }

    Ok(())
}

/// Write `value` to a cgroup virtual file. Bare write-only open without
/// `O_TRUNC` — cgroup interface files reject `O_TRUNC` (set by `fs::write`
/// and `OpenOptions::create+truncate`) with EINVAL on several kernels.
fn write_cgroup_file(path: &Path, value: &str) -> Result<(), CgroupError> {
    let mut f = fs::OpenOptions::new()
        .write(true)
        .open(path)
        .map_err(|source| CgroupError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    f.write_all(value.as_bytes())
        .map_err(|source| CgroupError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    Ok(())
}

fn pid_assignment_list(jailed: &JailedFirecracker) -> Vec<u32> {
    let mut pids = vec![jailed.jailer_pid, jailed.firecracker_pid];
    pids.sort_unstable();
    pids.dedup();
    pids
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn required_subtree_control_enables_three_controllers() {
        assert_eq!(REQUIRED_SUBTREE_CONTROL, "+cpu +memory +pids\n");
    }

    #[test]
    fn pid_assignment_sorts_and_deduplicates() {
        let jailed = JailedFirecracker {
            jailer_pid: 20,
            firecracker_pid: 10,
        };

        assert_eq!(pid_assignment_list(&jailed), vec![10, 20]);
    }

    #[test]
    fn pid_assignment_collapses_exec_equal_pids() {
        let jailed = JailedFirecracker {
            jailer_pid: 10,
            firecracker_pid: 10,
        };

        assert_eq!(pid_assignment_list(&jailed), vec![10]);
    }
}
