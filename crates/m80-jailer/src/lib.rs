//! Materialize the Firecracker jailer chroot per VM with a replayable plan.
//!
//! See `README.md` for the black-box contract.
//! Behavior captures: bead epic `m80-zmg` (`br show m80-zmg`).

#![deny(missing_docs)]

use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tracing::warn;

/// Name of the plan JSON file persisted in the run-dir.
pub const JAILER_PLAN_FILE: &str = "jailer-plan.json";
/// Name of the state JSON file persisted in the run-dir.
pub const JAILER_STATE_FILE: &str = "jailer-state.json";

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
    ///
    /// Steps are emitted in canonical order for deterministic replay:
    /// 1. `CreateDir` for the jail root.
    /// 2. `CreateDir` for each `CreateInsideJail` binding (in declared order).
    /// 3. `Bind` for each `Ro` or `Rw` binding (in declared order).
    /// 4. `Socket` for each socket spec (in declared order).
    pub fn compute(config: &JailerConfig) -> Result<Plan, JailerError> {
        if config.uid == 0 || config.gid == 0 {
            return Err(JailerError::UidGidInvalid {
                uid: config.uid,
                gid: config.gid,
            });
        }

        let jail_root = config.run_dir.join("jail");
        let mut steps = Vec::new();

        // Step 1: create the jail root.
        steps.push(PlanStep::CreateDir {
            path: jail_root.clone(),
            mode: 0o755,
        });

        // Step 2: CreateInsideJail entries (create dir, no source).
        for binding in &config.bindings {
            if binding.mode == BindMode::CreateInsideJail {
                steps.push(PlanStep::CreateDir {
                    path: jail_root.join(&binding.dest),
                    mode: 0o755,
                });
            }
        }

        // Step 3: Ro/Rw bind mounts.
        for binding in &config.bindings {
            if binding.mode == BindMode::Ro || binding.mode == BindMode::Rw {
                steps.push(PlanStep::Bind {
                    source: binding.source.clone(),
                    dest: jail_root.join(&binding.dest),
                    mode: binding.mode,
                });
            }
        }

        // Step 4: socket reservations.
        for spec in &config.sockets {
            steps.push(PlanStep::Socket {
                path: jail_root.join(&spec.path),
            });
        }

        Ok(Plan {
            config: config.clone(),
            steps,
        })
    }

    /// Materialize the plan: create dirs, perform binds, persist state.
    ///
    /// Requires `CAP_SYS_ADMIN` (or root). Persists `jailer-plan.json` and an
    /// initial `jailer-state.json` in the run-dir on success. `Drop` on the
    /// returned [`MaterializedJail`] tears down the chroot.
    pub fn materialize(&self) -> Result<MaterializedJail, JailerError> {
        use nix::mount::{MntFlags, MsFlags, mount};
        use nix::sys::stat::{Mode, fchmodat, FchmodatFlags};
        use nix::unistd::mkdir;

        let jail_root = self.config.run_dir.join("jail");

        // Construct the guard up-front so that on any `?`-propagated error the
        // partially-applied state (created dirs + bind mounts so far) gets
        // unwound by `Drop` instead of leaking into the host's mount table.
        let mut materialized = MaterializedJail {
            plan: self.clone(),
            jail_path: jail_root,
            bind_mounts: Vec::new(),
            created_dirs: Vec::new(),
            placeholder_files: Vec::new(),
        };

        for step in &self.steps {
            match step {
                PlanStep::CreateDir { path, .. } => {
                    mkdir(path, Mode::from_bits_truncate(0o755)).map_err(|e| {
                        JailerError::Io {
                            path: path.clone(),
                            source: io::Error::from_raw_os_error(e as i32),
                        }
                    })?;
                    fchmodat(
                        None,
                        path,
                        Mode::from_bits_truncate(0o755),
                        FchmodatFlags::FollowSymlink,
                    )
                    .map_err(|e| JailerError::Io {
                        path: path.clone(),
                        source: io::Error::from_raw_os_error(e as i32),
                    })?;
                    materialized.created_dirs.push(path.clone());
                }
                PlanStep::Bind { source, dest, mode } => {
                    if dest.is_dir() || source.is_dir() {
                        // dest dir was already created or is the jail root
                    } else {
                        std::fs::write(dest, b"").map_err(|source| JailerError::Io {
                            path: dest.clone(),
                            source,
                        })?;
                        materialized.placeholder_files.push(dest.clone());
                    }

                    mount(
                        Some(source.as_path()),
                        dest.as_path(),
                        None::<&str>,
                        MsFlags::MS_BIND,
                        None::<&str>,
                    )
                    .map_err(|_e| JailerError::BindFailed {
                        src: source.clone(),
                        dest: dest.clone(),
                    })?;
                    materialized.bind_mounts.push(dest.clone());

                    if *mode == BindMode::Ro {
                        mount(
                            None::<&str>,
                            dest.as_path(),
                            None::<&str>,
                            MsFlags::MS_BIND | MsFlags::MS_REMOUNT | MsFlags::MS_RDONLY,
                            None::<&str>,
                        )
                        .map_err(|_e| JailerError::BindFailed {
                            src: source.clone(),
                            dest: dest.clone(),
                        })?;
                    }
                }
                PlanStep::Socket { .. } => {
                    // Firecracker creates the UDS itself; nothing to do here.
                }
            }
        }

        let plan_path = self.config.run_dir.join(JAILER_PLAN_FILE);
        let plan_json =
            serde_json::to_vec_pretty(self).map_err(|e| JailerError::Io {
                path: plan_path.clone(),
                source: io::Error::new(io::ErrorKind::Other, e),
            })?;
        std::fs::write(&plan_path, &plan_json).map_err(|source| JailerError::Io {
            path: plan_path.clone(),
            source,
        })?;

        let state_path = self.config.run_dir.join(JAILER_STATE_FILE);
        let state = JailerState {
            jailer_pid: None,
            firecracker_pid: None,
        };
        let state_json =
            serde_json::to_vec_pretty(&state).map_err(|e| JailerError::Io {
                path: state_path.clone(),
                source: io::Error::new(io::ErrorKind::Other, e),
            })?;
        std::fs::write(&state_path, &state_json).map_err(|source| JailerError::Io {
            path: state_path.clone(),
            source,
        })?;

        let _ = MntFlags::empty(); // keep import alive for Drop impl below

        Ok(materialized)
    }
}

/// Serialized state for `jailer-state.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct JailerState {
    jailer_pid: Option<u32>,
    firecracker_pid: Option<u32>,
}

/// A materialized chroot. Drop tears it down.
#[derive(Debug)]
pub struct MaterializedJail {
    /// The plan that produced this jail.
    pub plan: Plan,
    /// Path to the chroot root.
    pub jail_path: PathBuf,
    /// Bind-mount destinations, in execution order (reversed on drop).
    bind_mounts: Vec<PathBuf>,
    /// Directories created (in execution order; reversed on drop).
    created_dirs: Vec<PathBuf>,
    /// Empty placeholder files we wrote to host file-bind targets. After
    /// the bind mount over a placeholder is removed in `Drop`, the
    /// placeholder itself becomes visible again and must be unlinked.
    placeholder_files: Vec<PathBuf>,
}

impl MaterializedJail {
    /// Exec `firecracker` inside the jail via the official `jailer` binary
    /// and return both pids tracked.
    ///
    /// Polls up to 1 s for `<jail_path>/firecracker.pid` to appear.
    pub fn launch(&self, api_socket: &Path) -> Result<JailedFirecracker, JailerError> {
        let vm_id = self
            .plan
            .config
            .run_dir
            .file_name()
            .ok_or_else(|| JailerError::ChrootFailed {
                jail_path: self.jail_path.clone(),
            })?
            .to_string_lossy()
            .into_owned();

        let chroot_base = self
            .jail_path
            .parent()
            .ok_or_else(|| JailerError::ChrootFailed {
                jail_path: self.jail_path.clone(),
            })?;

        let api_socket_name = api_socket
            .file_name()
            .ok_or_else(|| JailerError::ChrootFailed {
                jail_path: self.jail_path.clone(),
            })?;

        let mut child = Command::new(&self.plan.config.jailer_bin)
            .args([
                "--id",
                &vm_id,
                "--exec-file",
                &self.plan.config.firecracker_bin.to_string_lossy(),
                "--uid",
                &self.plan.config.uid.to_string(),
                "--gid",
                &self.plan.config.gid.to_string(),
                "--chroot-base-dir",
                &chroot_base.to_string_lossy(),
                "--api-sock",
                &api_socket_name.to_string_lossy(),
            ])
            .spawn()
            .map_err(|source| JailerError::Io {
                path: self.plan.config.jailer_bin.clone(),
                source,
            })?;

        let jailer_pid = child.id();

        // Wait up to 1 s for firecracker.pid to appear.
        let pid_file = self.jail_path.join("firecracker.pid");
        let deadline = Instant::now() + Duration::from_secs(1);
        let firecracker_pid = loop {
            if pid_file.exists() {
                let raw = std::fs::read_to_string(&pid_file).map_err(|source| {
                    JailerError::Io {
                        path: pid_file.clone(),
                        source,
                    }
                })?;
                let pid: u32 =
                    raw.trim()
                        .parse()
                        .map_err(|_| JailerError::ChrootFailed {
                            jail_path: self.jail_path.clone(),
                        })?;
                break pid;
            }
            if Instant::now() >= deadline {
                let _ = child.wait();
                return Err(JailerError::ChrootFailed {
                    jail_path: self.jail_path.clone(),
                });
            }
            thread::sleep(Duration::from_millis(25));
        };

        // Reap the jailer parent process. Jailer fork-execs firecracker into a
        // child and the parent exits quickly; without `wait()` it lingers as a
        // zombie until our process exits. The firecracker_pid we tracked above
        // is reparented to PID 1 and stays running.
        let _ = child.wait();

        // Persist updated state.
        let state_path = self.plan.config.run_dir.join(JAILER_STATE_FILE);
        let state = JailerState {
            jailer_pid: Some(jailer_pid),
            firecracker_pid: Some(firecracker_pid),
        };
        let state_json =
            serde_json::to_vec_pretty(&state).map_err(|e| JailerError::Io {
                path: state_path.clone(),
                source: io::Error::new(io::ErrorKind::Other, e),
            })?;
        std::fs::write(&state_path, &state_json).map_err(|source| JailerError::Io {
            path: state_path.clone(),
            source,
        })?;

        Ok(JailedFirecracker {
            jailer_pid,
            firecracker_pid,
        })
    }
}

impl Drop for MaterializedJail {
    fn drop(&mut self) {
        use nix::mount::{MntFlags, umount2};

        // Unmount bind mounts in reverse order.
        for path in self.bind_mounts.iter().rev() {
            if let Err(e) = umount2(path.as_path(), MntFlags::MNT_DETACH) {
                warn!("drop: umount2({}) failed: {e}", path.display());
            }
        }

        // Remove placeholder files we created for file-bind targets (now
        // visible again after umount).
        for path in self.placeholder_files.iter().rev() {
            if let Err(e) = std::fs::remove_file(path) {
                warn!("drop: unlink({}) failed: {e}", path.display());
            }
        }

        // Remove directories in reverse order.
        for path in self.created_dirs.iter().rev() {
            if let Err(e) = std::fs::remove_dir(path) {
                warn!("drop: rmdir({}) failed: {e}", path.display());
            }
        }
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
///
/// Reads `jailer-state.json` from `run_dir`. For each recorded PID, checks
/// whether `/proc/<pid>` exists. Returns:
/// - [`RecoveryDecision::NoJail`] if no state file is present.
/// - [`RecoveryDecision::LiveJail`] if both pids are alive.
/// - [`RecoveryDecision::OrphanJail`] otherwise (either or both pids dead),
///   with reap steps derived from `jailer-plan.json` in reverse order.
pub fn recover_from_run_dir(run_dir: &Path) -> Result<RecoveryDecision, JailerError> {
    let state_path = run_dir.join(JAILER_STATE_FILE);

    if !state_path.exists() {
        return Ok(RecoveryDecision::NoJail);
    }

    let raw = std::fs::read(&state_path).map_err(|source| JailerError::Io {
        path: state_path.clone(),
        source,
    })?;
    let state: JailerState =
        serde_json::from_slice(&raw).map_err(|e| JailerError::Io {
            path: state_path.clone(),
            source: io::Error::new(io::ErrorKind::InvalidData, e),
        })?;

    let jailer_alive = state
        .jailer_pid
        .map(pid_is_alive)
        .unwrap_or(false);
    let fc_alive = state
        .firecracker_pid
        .map(pid_is_alive)
        .unwrap_or(false);

    if jailer_alive && fc_alive {
        return Ok(RecoveryDecision::LiveJail {
            jailer_pid: state.jailer_pid.unwrap(),
            firecracker_pid: state.firecracker_pid.unwrap(),
        });
    }

    // Orphan or partial — load plan steps in reverse for reaping.
    let plan_path = run_dir.join(JAILER_PLAN_FILE);
    let reap_steps = if plan_path.exists() {
        let plan_raw = std::fs::read(&plan_path).map_err(|source| JailerError::Io {
            path: plan_path.clone(),
            source,
        })?;
        let plan: Plan =
            serde_json::from_slice(&plan_raw).map_err(|e| JailerError::Io {
                path: plan_path.clone(),
                source: io::Error::new(io::ErrorKind::InvalidData, e),
            })?;
        plan.steps.into_iter().rev().collect()
    } else {
        Vec::new()
    };

    Ok(RecoveryDecision::OrphanJail { reap_steps })
}

/// Returns `true` if `/proc/<pid>` exists (process is alive).
fn pid_is_alive(pid: u32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
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
    /// Underlying I/O failure; carries the path so the caller doesn't have to
    /// guess which file failed.
    #[error("i/o on {}: {source}", path.display())]
    Io {
        /// File the I/O was attempted against.
        path: PathBuf,
        /// Underlying I/O error.
        #[source]
        source: io::Error,
    },
}
