//! Per-VM cgroup v2 subtree: create, enforce CPU/memory/pids/I/O, clean up.
//!
//! See `README.md` for the black-box contract.
//! Behavior captures: bead epic `m80-84x` (`br show m80-84x`).

#![deny(missing_docs)]

use std::fs;
use std::io::{self, Write as IoWrite};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};
use tracing::warn;

use m80_jailer::{JailedFirecracker, MaterializedJail};

mod probe;

/// Root cgroup directory for all m80 VMs.
const CGROUP_ROOT: &str = "/sys/fs/cgroup/m80-firecracker";

/// Cgroup v2 global root.
const CGROUP_V2_ROOT: &str = "/sys/fs/cgroup";

const BASE_CONTROLLERS: &[&str] = &["cpu", "memory", "pids"];
const IO_CONTROLLER: &str = "io";
const DEFAULT_CPU_QUOTA_US: u64 = 100_000;
const DEFAULT_CPU_PERIOD_US: u64 = 100_000;
const DEFAULT_MEMORY_MAX_BYTES: u64 = 1_610_612_736;
const DEFAULT_PIDS_MAX: u32 = 128;
const DEFAULT_OOM_SCORE_ADJ: i16 = 500;

static SUBTREE_CONTROL_PRIMED: OnceLock<()> = OnceLock::new();
static SUBTREE_CONTROL_PRIME_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

/// One per-VM cgroup v2 subtree under `/sys/fs/cgroup/m80-firecracker/<vm-id>`.
#[derive(Debug)]
pub struct Subtree(PathBuf);

impl Subtree {
    /// Probe whether this host has unified cgroup v2 — call from preflight
    /// before any [`Subtree::create`].
    pub fn probe() -> Result<(), CgroupError> {
        probe::probe()
    }

    /// Create the per-VM subtree under `m80-firecracker/<vm_id>/`, enable
    /// needed controllers on every ancestor, apply limits, tune OOM preference,
    /// and then enroll both `jailed.jailer_pid` and `jailed.firecracker_pid`.
    /// Both PIDs are collected, sorted, and deduped before writing — after
    /// `m80-jailer` the jailer execs into firecracker so the two values are
    /// equal, and dedup handles that case correctly without omission.
    pub fn create(
        vm_id: &str,
        jail: &MaterializedJail,
        jailed: &JailedFirecracker,
        limits: &Limits,
    ) -> Result<Self, CgroupError> {
        let subtree = Self::create_at(
            Path::new(CGROUP_V2_ROOT),
            Path::new(CGROUP_ROOT),
            vm_id,
            jail.run_dir(),
            jailed,
            limits,
        )?;
        Ok(subtree)
    }

    fn create_at(
        base: &Path,
        parent: &Path,
        vm_id: &str,
        run_dir: &Path,
        jailed: &JailedFirecracker,
        limits: &Limits,
    ) -> Result<Self, CgroupError> {
        let io_err = |path: PathBuf| move |source| CgroupError::Io { path, source };

        // create_dir_all is race-safe against concurrent sandboxes.
        fs::create_dir_all(parent).map_err(io_err(parent.to_path_buf()))?;
        enable_subtree_control_for_create(base, parent, &limits.required_controllers())?;

        let leaf = parent.join(vm_id);
        fs::create_dir_all(&leaf).map_err(io_err(leaf.clone()))?;
        inherit_sparse_cpuset_file(parent, &leaf, "cpuset.cpus")?;
        inherit_sparse_cpuset_file(parent, &leaf, "cpuset.mems")?;

        let subtree = Subtree(leaf.clone());
        subtree.apply_limits(limits)?;
        if let Some(oom_score_adj) = limits.oom_score_adj {
            for pid in enrolled_pids(jailed.jailer_pid(), jailed.firecracker_pid()) {
                set_oom_score_adj(pid, oom_score_adj)?;
            }
        }

        let procs = leaf.join("cgroup.procs");
        for pid in enrolled_pids(jailed.jailer_pid(), jailed.firecracker_pid()) {
            write_cgroup_file(&procs, &format!("{pid}\n"))?;
        }

        let cgroup_path_txt = run_dir.join("cgroup-path.txt");
        let leaf_str = format!("{}\n", leaf.display());
        fs::write(&cgroup_path_txt, leaf_str.as_bytes()).map_err(io_err(cgroup_path_txt))?;

        Ok(subtree)
    }

    /// Apply per-controller limits. Fields set to `None` leave the existing
    /// value alone.
    pub(crate) fn apply_limits(&self, limits: &Limits) -> Result<(), CgroupError> {
        if let Some(cpu_max) = &limits.cpu_max {
            let path = self.0.join("cpu.max");
            match cpu_max {
                CpuMax::Quota {
                    quota_us,
                    period_us,
                } => write_cgroup_file(&path, &format!("{quota_us} {period_us}\n"))?,
                CpuMax::Max => write_cgroup_file(&path, "max\n")?,
            }
        }

        if let Some(mem) = limits.memory_max {
            write_cgroup_file(&self.0.join("memory.max"), &format!("{mem}\n"))?;
        }

        if let Some(pids) = limits.pids_max {
            write_cgroup_file(&self.0.join("pids.max"), &format!("{pids}\n"))?;
        }

        if let Some(io_weight) = limits.io_weight {
            validate_io_weight(io_weight)?;
            // Kernel default is 100; writing it is a no-op syscall, skip it.
            if io_weight != 100 {
                write_cgroup_file(&self.0.join("io.weight"), &format!("default {io_weight}\n"))?;
            }
        }

        for io_max in &limits.io_max {
            write_cgroup_file(&self.0.join("io.max"), &format!("{io_max}\n"))?;
        }

        Ok(())
    }

    /// Compute the absolute cgroup v2 leaf path for `vm_id`.
    #[must_use]
    pub fn leaf_path(vm_id: &str) -> PathBuf {
        PathBuf::from(CGROUP_ROOT).join(vm_id)
    }
}

fn enrolled_pids(jailer_pid: u32, firecracker_pid: u32) -> Vec<u32> {
    let mut pids = vec![jailer_pid, firecracker_pid];
    pids.retain(|pid| *pid != 0);
    pids.sort_unstable();
    pids.dedup();
    pids
}

impl Drop for Subtree {
    fn drop(&mut self) {
        if let Err(e) = kill_cgroup(&self.0) {
            warn!("drop: cgroup.kill({}) failed: {e}", self.0.display());
        }
        if let Err(e) = fs::remove_dir(&self.0) {
            warn!("drop: rmdir({}) failed: {e}", self.0.display());
        }
    }
}

/// Best-effort cleanup of a stale subtree from a prior crashed run.
///
/// If the directory does not exist, returns `Ok(())`. If it exists and
/// `cgroup.procs` is non-empty, returns `Err(CgroupError::LivePids)`.
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
        return Err(CgroupError::LivePids {
            path: leaf,
            pids: procs.trim().to_owned(),
        });
    }

    fs::remove_dir(&leaf).map_err(|source| CgroupError::Io { path: leaf, source })?;

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
    /// `io.max` device throttle lines. Empty = leave existing.
    #[serde(default)]
    pub io_max: Vec<IoMax>,
    /// cgroup v2 `io.weight` default. None = leave existing.
    pub io_weight: Option<u16>,
    /// `/proc/<pid>/oom_score_adj` for enrolled jailed processes.
    pub oom_score_adj: Option<i16>,
}

impl Limits {
    /// m80's default VM resource limit profile.
    ///
    /// CPU is one full 100 ms CPU period, memory is 1.5 GiB, and pids are
    /// capped at 128.
    #[must_use]
    pub fn preset() -> Self {
        Self {
            cpu_max: Some(CpuMax::Quota {
                quota_us: DEFAULT_CPU_QUOTA_US,
                period_us: DEFAULT_CPU_PERIOD_US,
            }),
            memory_max: Some(DEFAULT_MEMORY_MAX_BYTES),
            pids_max: Some(DEFAULT_PIDS_MAX),
            io_max: Vec::new(),
            io_weight: None,
            oom_score_adj: Some(DEFAULT_OOM_SCORE_ADJ),
        }
    }

    fn required_controllers(&self) -> Vec<&'static str> {
        let mut controllers = BASE_CONTROLLERS.to_vec();
        if self.io_weight.is_some() || !self.io_max.is_empty() {
            controllers.push(IO_CONTROLLER);
        }
        controllers
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

/// One cgroup v2 `io.max` throttle row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IoMax {
    /// Block-device major number.
    pub major: u32,
    /// Block-device minor number.
    pub minor: u32,
    /// Optional read bytes-per-second throttle.
    pub rbps: Option<u64>,
    /// Optional write bytes-per-second throttle.
    pub wbps: Option<u64>,
    /// Optional read IOPS throttle.
    pub riops: Option<u64>,
    /// Optional write IOPS throttle.
    pub wiops: Option<u64>,
}

impl std::fmt::Display for IoMax {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.major, self.minor)?;
        if let Some(value) = self.rbps {
            write!(f, " rbps={value}")?;
        }
        if let Some(value) = self.wbps {
            write!(f, " wbps={value}")?;
        }
        if let Some(value) = self.riops {
            write!(f, " riops={value}")?;
        }
        if let Some(value) = self.wiops {
            write!(f, " wiops={value}")?;
        }
        Ok(())
    }
}

/// Errors surfaced by cgroup operations.
#[derive(Debug, thiserror::Error)]
pub enum CgroupError {
    /// Host is not in unified cgroup v2 mode.
    #[error("host is not in unified cgroup v2 mode")]
    UnsupportedHostMode,
    /// A required cgroup v2 controller is not enabled in the parent.
    #[error("controller not enabled: {0}")]
    ControllerNotEnabled(&'static str),
    /// A cgroup file that must inherit from an ancestor had no non-empty value.
    #[error("sparse cgroup file has no non-empty ancestor: {0}")]
    SparseInheritedFile(&'static str),
    /// A limit value is outside the kernel-accepted range.
    #[error("invalid cgroup limit {field}: {value}")]
    InvalidLimit {
        /// Field name.
        field: &'static str,
        /// Invalid value.
        value: String,
    },
    /// Subtree still has live PIDs; cannot clean up.
    #[error("subtree {} still has live pids: {pids}", path.display())]
    LivePids {
        /// Subtree path.
        path: PathBuf,
        /// Raw content of `cgroup.procs`.
        pids: String,
    },
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

fn kill_cgroup(path: &Path) -> Result<(), CgroupError> {
    let kill_path = path.join("cgroup.kill");
    if !kill_path.exists() {
        return Ok(());
    }
    write_cgroup_file(&kill_path, "1\n")
}

fn enable_subtree_control_chain(
    base: &Path,
    parent: &Path,
    controllers: &[&'static str],
) -> Result<(), CgroupError> {
    let mut path = base.to_path_buf();
    write_subtree_control_checked(&path, controllers)?;

    let relative = parent.strip_prefix(base).unwrap_or(parent);
    for component in relative.components() {
        path.push(component.as_os_str());
        write_subtree_control_unchecked(&path, controllers)?;
    }
    Ok(())
}

fn enable_subtree_control_for_create(
    base: &Path,
    parent: &Path,
    controllers: &[&'static str],
) -> Result<(), CgroupError> {
    if base == Path::new(CGROUP_V2_ROOT) && parent == Path::new(CGROUP_ROOT) {
        enable_subtree_control_chain_once(&SUBTREE_CONTROL_PRIMED, || {
            enable_subtree_control_chain(base, parent, controllers)
        })
    } else {
        enable_subtree_control_chain(base, parent, controllers)
    }
}

fn enable_subtree_control_chain_once<F>(primed: &OnceLock<()>, enable: F) -> Result<(), CgroupError>
where
    F: FnOnce() -> Result<(), CgroupError>,
{
    if primed.get().is_some() {
        return Ok(());
    }

    let prime_lock = SUBTREE_CONTROL_PRIME_LOCK.get_or_init(|| Mutex::new(()));
    let _guard = prime_lock
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if primed.get().is_some() {
        return Ok(());
    }

    enable()?;
    let _ = primed.set(());
    Ok(())
}

fn write_subtree_control_checked(
    path: &Path,
    controllers: &[&'static str],
) -> Result<(), CgroupError> {
    let controllers_path = path.join("cgroup.controllers");
    let available = fs::read_to_string(&controllers_path).map_err(|source| CgroupError::Io {
        path: controllers_path.clone(),
        source,
    })?;
    for controller in controllers {
        if !available.split_whitespace().any(|c| c == *controller) {
            return Err(CgroupError::ControllerNotEnabled(controller));
        }
    }

    write_subtree_control_unchecked(path, controllers)
}

fn write_subtree_control_unchecked(
    path: &Path,
    controllers: &[&'static str],
) -> Result<(), CgroupError> {
    let value = controllers
        .iter()
        .map(|controller| format!("+{controller}"))
        .collect::<Vec<_>>()
        .join(" ");
    write_cgroup_file(&path.join("cgroup.subtree_control"), &(value + "\n"))
}

fn inherit_sparse_cpuset_file(
    parent: &Path,
    leaf: &Path,
    filename: &'static str,
) -> Result<(), CgroupError> {
    let leaf_file = leaf.join(filename);
    if !leaf_file.exists() {
        return Ok(());
    }
    let current = fs::read_to_string(&leaf_file).map_err(|source| CgroupError::Io {
        path: leaf_file.clone(),
        source,
    })?;
    if !current.trim().is_empty() {
        return Ok(());
    }

    let mut cursor = Some(parent);
    while let Some(path) = cursor {
        let candidate = path.join(filename);
        if candidate.exists() {
            let inherited = fs::read_to_string(&candidate).map_err(|source| CgroupError::Io {
                path: candidate.clone(),
                source,
            })?;
            if !inherited.trim().is_empty() {
                return write_cgroup_file(&leaf_file, &inherited);
            }
        }
        cursor = path.parent();
    }

    Err(CgroupError::SparseInheritedFile(filename))
}

fn validate_io_weight(value: u16) -> Result<(), CgroupError> {
    if (1..=10_000).contains(&value) {
        Ok(())
    } else {
        Err(CgroupError::InvalidLimit {
            field: "io_weight",
            value: value.to_string(),
        })
    }
}

fn set_oom_score_adj(pid: u32, value: i16) -> Result<(), CgroupError> {
    if !(-1000..=1000).contains(&value) {
        return Err(CgroupError::InvalidLimit {
            field: "oom_score_adj",
            value: value.to_string(),
        });
    }
    let path = PathBuf::from("/proc")
        .join(pid.to_string())
        .join("oom_score_adj");
    fs::write(&path, format!("{value}\n")).map_err(|source| CgroupError::Io { path, source })
}

#[cfg(test)]
mod tests;
