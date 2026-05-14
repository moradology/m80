//! Plan-shape types + persistence-file constants + chroot path computation.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::JailerError;

/// Name of the plan JSON file persisted in the run-dir.
pub const JAILER_PLAN_FILE: &str = "jailer-plan.json";
/// Name of the state JSON file persisted in the run-dir.
pub const JAILER_STATE_FILE: &str = "jailer-state.json";

/// Caller-supplied configuration for one jailed VM.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JailerConfig {
    /// Absolute path to the `jailer` binary.
    pub jailer_bin: PathBuf,
    /// Absolute path to the m80 process-hardening wrapper that execs the
    /// official jailer after applying inherited one-way hardening.
    pub jailer_harden_bin: Option<PathBuf>,
    /// Absolute path to the `firecracker` binary.
    pub firecracker_bin: PathBuf,
    /// Per-VM run directory. Used as jailer's `--chroot-base-dir`. The
    /// directory's basename is used as `--id`, so the actual chroot ends
    /// up at `<run_dir>/<firecracker_bin basename>/<run_dir basename>/root/`
    /// (jailer's hardcoded layout — we cannot pick a different leaf).
    pub run_dir: PathBuf,
    /// UID inside the jail.
    pub uid: u32,
    /// GID inside the jail.
    pub gid: u32,
    /// Mounts to bind into the jail (RO, RW, or "create inside").
    pub bindings: Vec<Binding>,
    /// Sockets to reserve inside the jail (API socket and vsock UDS).
    pub sockets: Vec<JailerSocket>,
    /// Resource limits the official jailer applies before exec'ing
    /// firecracker.
    pub resource_limits: ResourceLimits,
    /// Ask the official jailer to launch firecracker as PID 1 in a new PID
    /// namespace.
    pub new_pid_ns: bool,
    /// Ask the official jailer to double-fork before exec'ing Firecracker.
    /// When set, `jailer_pid` is recorded as `0` because no live jailer parent
    /// remains for m80 to signal.
    pub daemonize: bool,
    /// Ask `m80-jailer-harden` to enter a private cgroup namespace before
    /// execing the official jailer.
    pub new_cgroup_ns: bool,
    /// Optional caller-provided network namespace path passed to the official
    /// jailer as `--netns`.
    pub netns_path: Option<PathBuf>,
    /// Optional Firecracker advanced seccomp filter path inside the jail.
    pub seccomp_filter_path: Option<PathBuf>,
    /// Optional host-side file that receives firecracker/jailer stdout and
    /// stderr. The orchestrator uses this for the per-VM serial console log.
    pub stdio_log: Option<PathBuf>,
}

/// Per-VM process resource limits passed through to Firecracker's official
/// jailer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceLimits {
    /// Maximum number of open file descriptors.
    pub no_file: u64,
    /// Optional maximum size, in bytes, of files created by the process.
    pub fsize: Option<u64>,
    /// Optional maximum number of processes for the process real uid.
    pub nproc: Option<u64>,
    /// Optional maximum bytes that may be locked into memory.
    pub memlock: Option<u64>,
    /// Optional maximum process address space in bytes.
    pub address_space: Option<u64>,
    /// Optional maximum core file size in bytes.
    pub core: Option<u64>,
    /// Optional maximum stack size in bytes.
    pub stack: Option<u64>,
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            no_file: 2048,
            fsize: None,
            nproc: None,
            memlock: None,
            address_space: None,
            core: Some(0),
            stack: Some(8 * 1024 * 1024),
        }
    }
}

/// Compute the actual jail root path inside `run_dir`.
///
/// Firecracker's jailer hardcodes the nested layout
/// `<chroot-base>/<exec-file basename>/<id>/root/`; m80 passes `run_dir` for
/// `--chroot-base-dir` and `run_dir`'s basename for `--id`.
///
/// # Panics
///
/// Panics if either path lacks a final component. Callers that accept
/// user-supplied paths should validate first with [`check_plan_basenames`].
#[must_use]
pub fn jail_root_path(run_dir: &Path, firecracker_bin: &Path) -> PathBuf {
    let exec_basename = firecracker_bin
        .file_name()
        .expect("firecracker_bin has no basename");
    let id_basename = run_dir.file_name().expect("run_dir has no basename");
    run_dir.join(exec_basename).join(id_basename).join("root")
}

/// Validate that both paths required by [`jail_root_path`] have a final
/// component. Returns [`JailerError::NoBasename`] for the first offender.
pub(crate) fn check_plan_basenames(
    run_dir: &Path,
    firecracker_bin: &Path,
) -> Result<(), JailerError> {
    if firecracker_bin.file_name().is_none() {
        return Err(JailerError::NoBasename {
            path: firecracker_bin.to_path_buf(),
        });
    }
    if run_dir.file_name().is_none() {
        return Err(JailerError::NoBasename {
            path: run_dir.to_path_buf(),
        });
    }
    Ok(())
}

/// One bind-mount (or in-jail directory) the jailer must materialize.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    /// Source path on the host.
    pub source: PathBuf,
    /// Destination path inside the jail.
    pub dest: PathBuf,
    /// How the destination is materialized.
    pub mode: BindMode,
}

/// How a [`Binding`]'s destination is created.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub enum BindMode {
    /// Bind read-only.
    Ro,
    /// Bind read-write.
    Rw,
    /// Create the destination directory inside the jail (no host source).
    CreateInsideJail,
}

/// One of the two UDS sockets the jailer creates inside the jail.
///
/// Each variant carries its canonical relative path; there is no variable-path
/// data in this type because the two paths are hardcoded by the m80
/// orchestration layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum JailerSocket {
    /// Firecracker's REST API socket (`firecracker.sock`).
    Firecracker,
    /// The virtio-vsock host-side socket (`vsock.sock`).
    Vsock,
}

impl JailerSocket {
    /// Canonical relative path inside the jail for this socket.
    pub(crate) fn jail_path(self) -> &'static str {
        match self {
            JailerSocket::Firecracker => "firecracker.sock",
            JailerSocket::Vsock => "vsock.sock",
        }
    }
}

/// Pure description of every filesystem step a materialize would take.
/// Replayable for offline triage.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    /// Format version. Current: 1. Present so `deny_unknown_fields` does not
    /// permanently prevent adding fields to the persisted plan.
    #[serde(default = "plan_version")]
    pub(crate) schema_version: u32,
    /// The config the plan was derived from.
    pub(crate) config: JailerConfig,
    /// Ordered steps the materializer will execute.
    pub(crate) steps: Vec<PlanStep>,
}

fn plan_version() -> u32 {
    1
}

/// One step in a [`Plan`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, tag = "kind", rename_all = "snake_case")]
pub(crate) enum PlanStep {
    /// Create a directory at `path` with the given mode.
    CreateDir {
        /// Path to create.
        path: PathBuf,
        /// Unix mode bits.
        mode: u32,
    },
    /// Bind-mount `source` at `dest` with `mode`.
    Bind {
        /// Host source path.
        source: PathBuf,
        /// In-jail destination path.
        dest: PathBuf,
        /// Bind mode (RO/RW).
        mode: BindMode,
    },
    /// Reserve a UDS socket path inside the jail.
    Socket {
        /// In-jail socket path.
        path: PathBuf,
    },
}

/// Serialized state for `jailer-state.json`. Crate-internal — the
/// public surface uses [`JailedFirecracker`](super::JailedFirecracker)
/// and [`InspectionDecision`](super::InspectionDecision).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct JailerState {
    /// Format version. Current: 1. Present so `deny_unknown_fields` does not
    /// permanently prevent adding fields to persisted state.
    #[serde(default = "jailer_state_version")]
    pub(crate) schema_version: u32,
    pub(crate) jailer_pid: Option<u32>,
    pub(crate) firecracker_pid: Option<u32>,
}

fn jailer_state_version() -> u32 {
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_resource_limits_do_not_set_host_wide_nproc() {
        assert_eq!(ResourceLimits::default().nproc, None);
    }

    #[test]
    fn default_resource_limits_leave_memlock_inherited() {
        assert_eq!(ResourceLimits::default().memlock, None);
    }
}
