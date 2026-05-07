//! Plan-shape types + persistence-file constants + chroot path computation.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

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
    /// Sockets to create inside the jail (e.g., the API and vsock UDSes).
    pub sockets: Vec<SocketSpec>,
    /// Optional host-side file that receives firecracker/jailer stdout and
    /// stderr. The orchestrator uses this for the per-VM serial console log.
    pub stdio_log: Option<PathBuf>,
}

/// Compute the actual jail root path inside `run_dir`.
///
/// Firecracker's jailer hardcodes the nested layout
/// `<chroot-base>/<exec-file basename>/<id>/root/`; m80 passes `run_dir` for
/// `--chroot-base-dir` and `run_dir`'s basename for `--id`.
pub fn jail_root_path(run_dir: &Path, firecracker_bin: &Path) -> PathBuf {
    let exec_basename = firecracker_bin
        .file_name()
        .unwrap_or_else(|| std::ffi::OsStr::new("firecracker"));
    let id_basename = run_dir
        .file_name()
        .unwrap_or_else(|| std::ffi::OsStr::new("vm"));
    run_dir.join(exec_basename).join(id_basename).join("root")
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

/// Path inside the jail where a UDS will be created at materialization time.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SocketSpec {
    /// Path inside the jail (e.g., `firecracker.sock`).
    pub path: PathBuf,
}

/// Pure description of every filesystem step a materialize would take.
/// Replayable for offline triage.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    /// The config the plan was derived from.
    pub config: JailerConfig,
    /// Ordered steps the materializer will execute.
    pub steps: Vec<PlanStep>,
}

/// One step in a [`Plan`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, tag = "kind", rename_all = "snake_case")]
pub enum PlanStep {
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
    pub(crate) jailer_pid: Option<u32>,
    pub(crate) firecracker_pid: Option<u32>,
}
