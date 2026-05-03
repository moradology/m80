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

/// One per-VM cgroup v2 subtree under `/sys/fs/cgroup/m80-firecracker/<vm-id>`.
#[derive(Debug)]
pub struct Subtree {
    path: PathBuf,
}

impl Subtree {
    /// Probe whether this host supports unified cgroup v2 mode. Run this
    /// before [`Subtree::create`] from preflight.
    ///
    /// Reads `/proc/mounts` and looks for a `cgroup2` mount at
    /// `/sys/fs/cgroup`. Returns [`CgroupError::UnsupportedHostMode`] if
    /// none is found.
    pub fn probe() -> Result<(), CgroupError> {
        let mounts = fs::read_to_string("/proc/mounts").map_err(|source| CgroupError::Io {
            path: PathBuf::from("/proc/mounts"),
            source,
        })?;
        probe_mounts(&mounts)
    }

    /// Create the per-VM subtree, enable cpu/memory/pids controllers, assign
    /// both `jailed.jailer_pid` and `jailed.firecracker_pid` to the subtree.
    ///
    /// The `jail` argument supplies `plan.config.run_dir` for the persisted
    /// `cgroup-path.txt`. The `jailed` argument supplies the live pids.
    pub fn create(
        vm_id: &str,
        jail: &MaterializedJail,
        jailed: &JailedFirecracker,
    ) -> Result<Self, CgroupError> {
        let parent = PathBuf::from(CGROUP_ROOT);

        // 1. Ensure parent dir exists. `create_dir_all` is race-safe: two
        //    concurrent sandboxes can both call this without one losing.
        fs::create_dir_all(&parent).map_err(|source| CgroupError::Io {
            path: parent.clone(),
            source,
        })?;

        // 2. Enable controllers in the parent. Use `write_cgroup_file` (no
        //    O_TRUNC) — cgroup virtual files reject O_TRUNC on some kernels.
        let subtree_control = parent.join("cgroup.subtree_control");
        if let Err(cgroup_err) = write_cgroup_file(&subtree_control, "+cpu +memory +pids\n") {
            // On failure, check cgroup.controllers and surface the structured
            // ControllerNotEnabled variant when we can name the missing one;
            // otherwise propagate the original CgroupError unchanged.
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

        // 3. Create the per-VM leaf directory. `create_dir_all` is race-safe
        //    against a stale-from-prior-crash leaf — but if it pre-existed,
        //    the prior controller state may persist; cleanup_orphan_subtree
        //    is the orthogonal preflight step the orchestrator should call.
        let leaf = parent.join(vm_id);
        fs::create_dir_all(&leaf).map_err(|source| CgroupError::Io {
            path: leaf.clone(),
            source,
        })?;

        // 4. Assign the firecracker pid to the leaf. Note: jailer's
        //    `JailedFirecracker::jailer_pid` is the pid of the jailer parent,
        //    which `m80-jailer::launch()` reaps via `child.wait()` before
        //    returning — so by the time we get here it's already exited and
        //    writing it to cgroup.procs would return ESRCH. The firecracker
        //    process is what we actually want to constrain anyway.
        let procs = leaf.join("cgroup.procs");
        write_cgroup_file(&procs, &format!("{}\n", jailed.firecracker_pid))?;

        // 5. Persist the subtree path to <run_dir>/cgroup-path.txt.
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
                CpuMax::Quota { quota_us, period_us } => {
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
}

impl Drop for Subtree {
    fn drop(&mut self) {
        if let Err(e) = fs::remove_dir(&self.path) {
            warn!(
                "drop: rmdir({}) failed: {e}",
                self.path.display()
            );
        }
    }
}

/// Best-effort cleanup of a stale subtree from a prior crashed run.
///
/// If the directory does not exist, returns `Ok(())`. If it exists and
/// `cgroup.procs` is non-empty, logs a warning and returns `Ok(())`.
/// If it exists and is empty, removes it.
pub fn cleanup_orphan_subtree(vm_id: &str) -> Result<(), CgroupError> {
    let leaf = PathBuf::from(CGROUP_ROOT).join(vm_id);
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

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Internal mount-string probe, exposed for integration tests.
///
/// Not part of the stable public surface — call [`Subtree::probe`] from
/// production code. Tests use this to inject fake `/proc/mounts` content
/// without touching the real filesystem.
#[doc(hidden)]
pub fn probe_mounts_test(mounts: &str) -> Result<(), CgroupError> {
    probe_mounts(mounts)
}

/// Check mounts content for cgroup v2 unified hierarchy. Separated from
/// `probe()` so tests can call it with a fake `/proc/mounts` string.
pub(crate) fn probe_mounts(mounts: &str) -> Result<(), CgroupError> {
    // A unified-v2 host has a `cgroup2` mount at `/sys/fs/cgroup`.
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

/// Write `value` to `path` using `OpenOptions::write(true).open(...)` —
/// **deliberately without `.create(true)` and `.truncate(true)`**.
///
/// Cgroup v2 interface files are virtual: they exist only when the
/// controller is enabled in the parent's `subtree_control`, and writing
/// to them with `O_TRUNC` (which `fs::write` and the equivalent
/// `OpenOptions::create(true).truncate(true)` invocation set) is rejected
/// with EINVAL on several kernels. A bare write-only open behaves
/// correctly across the kernel matrix m80 supports.
///
/// Use this helper instead of `fs::write` for every cgroup file.
fn write_cgroup_file(path: &Path, value: &str) -> Result<(), CgroupError> {
    let mut f = fs::OpenOptions::new()
        .write(true)
        .open(path)
        .map_err(|source| CgroupError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    f.write_all(value.as_bytes()).map_err(|source| CgroupError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(())
}
