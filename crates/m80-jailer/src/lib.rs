//! Materialize the Firecracker jailer chroot per VM with a replayable plan.
//!
//! See `README.md` for the black-box contract.
//! Behavior captures: bead epic `m80-zmg` (`br show m80-zmg`).
//!
//! # Type-pinning pass
//!
//! Public surface is declared here; bodies are `todo!()`. Implementation lands
//! in a later wave.

#![deny(missing_docs)]

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Caller-supplied configuration for one jailed VM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JailerConfig {
    /// Absolute path to the `jailer` binary.
    pub jailer_bin: PathBuf,
    /// Absolute path to the `firecracker` binary.
    pub firecracker_bin: PathBuf,
    /// Per-VM run directory. The jail is materialized under
    /// `<run_dir>/jail/`.
    pub run_dir: PathBuf,
    /// UID inside the jail.
    pub uid: u32,
    /// GID inside the jail.
    pub gid: u32,
    /// Mounts to bind into the jail (RO, RW, or "create inside").
    pub bindings: Vec<Binding>,
    /// Sockets to create inside the jail (e.g., the API and vsock UDSes).
    pub sockets: Vec<SocketSpec>,
}

/// One bind-mount (or in-jail directory) the jailer must materialize.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
#[serde(rename_all = "snake_case")]
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
pub struct SocketSpec {
    /// Path inside the jail (e.g., `firecracker.sock`).
    pub path: PathBuf,
}

/// Pure description of every filesystem step a materialize would take.
/// Replayable for offline triage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    /// The config the plan was derived from.
    pub config: JailerConfig,
    /// Ordered steps the materializer will execute.
    pub steps: Vec<PlanStep>,
}

/// One step in a [`Plan`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
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

impl Plan {
    /// Compute a plan from a config without touching the filesystem.
    pub fn compute(_config: &JailerConfig) -> Result<Plan, JailerError> {
        todo!()
    }

    /// Materialize the plan: create dirs, perform binds, exec the jailer.
    pub fn materialize(&self) -> Result<MaterializedJail, JailerError> {
        todo!()
    }
}

/// A materialized chroot. Drop tears it down.
#[derive(Debug)]
pub struct MaterializedJail {
    /// The plan that produced this jail.
    pub plan: Plan,
    /// Path to the chroot root.
    pub jail_path: PathBuf,
}

impl MaterializedJail {
    /// Exec `firecracker` inside the jail and return both pids tracked.
    pub fn launch(&self, _api_socket: &Path) -> Result<JailedFirecracker, JailerError> {
        todo!()
    }
}

/// A live jailed firecracker process.
#[derive(Debug)]
pub struct JailedFirecracker {
    /// PID of the `jailer` process itself.
    pub jailer_pid: u32,
    /// PID of the `firecracker` child the jailer exec'd.
    pub firecracker_pid: u32,
}

/// Outcome of [`recover_from_run_dir`].
#[derive(Debug, Clone)]
pub enum RecoveryDecision {
    /// A live jailer + firecracker pair was found.
    LiveJail {
        /// PID of the live jailer.
        jailer_pid: u32,
        /// PID of the live firecracker child.
        firecracker_pid: u32,
    },
    /// A stale jail was found; reaping is needed.
    OrphanJail {
        /// Steps the caller should run to clean up.
        reap_steps: Vec<PlanStep>,
    },
    /// No jail was found at this run-dir.
    NoJail,
}

/// Inspect a run-dir for prior jail state and decide what to do with it.
pub fn recover_from_run_dir(_run_dir: &Path) -> Result<RecoveryDecision, JailerError> {
    todo!()
}

/// Errors surfaced by jailer operations.
#[derive(Debug, thiserror::Error)]
pub enum JailerError {
    /// Insufficient privilege to invoke the jailer.
    #[error("insufficient privilege to launch jailer")]
    LaunchPrivilegeUnavailable,
    /// A bind-mount failed.
    #[error("bind-mount failed: src={src} dest={dest}", src = src.display(), dest = dest.display())]
    BindFailed {
        /// Host source path that failed.
        src: PathBuf,
        /// In-jail destination path that failed.
        dest: PathBuf,
    },
    /// `chroot` syscall (or jailer's chroot step) failed.
    #[error("chroot failed in {jail_path}", jail_path = jail_path.display())]
    ChrootFailed {
        /// Jail path that failed to chroot.
        jail_path: PathBuf,
    },
    /// UID/GID was rejected (out of range or unknown).
    #[error("invalid uid/gid: uid={uid} gid={gid}")]
    UidGidInvalid {
        /// UID that was rejected.
        uid: u32,
        /// GID that was rejected.
        gid: u32,
    },
    /// Underlying I/O failure.
    #[error("i/o: {0}")]
    Io(#[from] io::Error),
}
