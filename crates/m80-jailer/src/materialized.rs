//! `MaterializedJail` (handle returned by `Plan::materialize`),
//! `MaterializedJail::launch` (jailer exec), `Drop` (chroot teardown),
//! and `JailedFirecracker` (live pids).

use std::io;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use tracing::warn;

use crate::error::JailerError;
use crate::plan::write_file_no_follow;
use crate::types::{JailerState, Plan, JAILER_STATE_FILE};

const FIRECRACKER_PID_TIMEOUT: Duration = Duration::from_secs(1);
const FIRECRACKER_PID_INITIAL_POLL: Duration = Duration::from_millis(1);
const FIRECRACKER_PID_MAX_POLL: Duration = Duration::from_millis(25);

/// A materialized chroot. Drop tears it down.
#[derive(Debug)]
pub struct MaterializedJail {
    /// The plan that produced this jail.
    pub(crate) plan: Plan,
    /// Path to the chroot root.
    pub(crate) jail_path: PathBuf,
    /// Bind-mount destinations, in execution order (reversed on drop).
    pub(crate) bind_mounts: Vec<PathBuf>,
    /// Directories created (in execution order; reversed on drop).
    pub(crate) created_dirs: Vec<PathBuf>,
    /// Empty placeholder files we wrote to host file-bind targets. After
    /// the bind mount over a placeholder is removed in `Drop`, the
    /// placeholder itself becomes visible again and must be unlinked.
    pub(crate) placeholder_files: Vec<PathBuf>,
}

impl MaterializedJail {
    /// Path to the actual chroot root created for this jail.
    #[must_use]
    pub fn jail_root(&self) -> &Path {
        &self.jail_path
    }

    /// Per-VM run directory that owns the persisted jailer plan and state.
    #[must_use]
    pub fn run_dir(&self) -> &Path {
        &self.plan.config.run_dir
    }

    /// Exec `firecracker` inside the jail via the official `jailer` binary
    /// and return both pids tracked.
    ///
    /// Polls up to 1 s for `<jail_path>/firecracker.pid` to appear.
    pub fn launch(&self, api_socket: &Path) -> Result<JailedFirecracker, JailerError> {
        // run_dir is `<run_root>/<vm_id>` and api_socket is `firecracker.sock`
        // — both come from m80-firecracker's known shape, so a missing
        // file_name here is a programmer bug, not a runtime fault.
        let vm_id = self
            .plan
            .config
            .run_dir
            .file_name()
            .expect("run_dir has a basename")
            .to_string_lossy()
            .into_owned();
        let api_socket_name = api_socket.file_name().expect("api_socket has a basename");

        // jailer's `--chroot-base-dir` is the run_dir; jailer appends
        // `<exec basename>/<id>/root/` to derive the real chroot
        // (matches `jail_root_path()` in `types.rs`).
        let chroot_base = self.plan.config.run_dir.as_path();

        // Jailer args end at the bare `--`; everything after is forwarded
        // to the jailed firecracker binary. `--api-sock` is firecracker's
        // arg, not jailer's, so it goes on the right side of the separator.
        //
        // Without `--daemonize` or `--new-pid-ns`, jailer `exec()`s into
        // firecracker, so this `Child` handle's pid IS the firecracker pid.
        // We never `wait()` on that handle (that would block until the VM
        // exits). Drop on `JailedFirecracker` is responsible for kill+reap.
        //
        // With `--daemonize` or `--new-pid-ns`, the official jailer parent
        // exits after writing `firecracker.pid`; m80 records jailer_pid = 0 as
        // the no-live-jailer-parent sentinel.
        //
        let (command_path, mut command) =
            if let Some(jailer_harden_bin) = &self.plan.config.jailer_harden_bin {
                let mut command = Command::new(jailer_harden_bin);
                command
                    .arg("--jailer-bin")
                    .arg(&self.plan.config.jailer_bin)
                    .arg("--uid")
                    .arg(self.plan.config.uid.to_string())
                    .arg("--gid")
                    .arg(self.plan.config.gid.to_string());
                push_extended_resource_limits(&mut command, &self.plan.config.resource_limits);
                if self.plan.config.new_cgroup_ns {
                    command.arg("--new-cgroup-ns");
                }
                command.arg("--");
                (jailer_harden_bin, command)
            } else {
                (
                    &self.plan.config.jailer_bin,
                    Command::new(&self.plan.config.jailer_bin),
                )
            };

        command
            .env_clear()
            .arg("--id")
            .arg(&vm_id)
            .arg("--exec-file")
            .arg(&self.plan.config.firecracker_bin)
            .arg("--uid")
            .arg(self.plan.config.uid.to_string())
            .arg("--gid")
            .arg(self.plan.config.gid.to_string())
            .arg("--chroot-base-dir")
            .arg(chroot_base)
            .arg("--resource-limit")
            .arg(format!(
                "no-file={}",
                self.plan.config.resource_limits.no_file
            ));

        if let Some(fsize) = self.plan.config.resource_limits.fsize {
            command
                .arg("--resource-limit")
                .arg(format!("fsize={fsize}"));
        }

        if self.plan.config.new_pid_ns {
            command.arg("--new-pid-ns");
        }

        if self.plan.config.daemonize {
            command.arg("--daemonize");
        }

        if let Some(netns_path) = &self.plan.config.netns_path {
            validate_netns_path(netns_path)?;
            command.arg("--netns").arg(netns_path);
        }

        command.arg("--").arg("--api-sock").arg(api_socket_name);

        command.stdin(Stdio::null());

        if let Some(stdio_log) = &self.plan.config.stdio_log {
            let file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(stdio_log)
                .map_err(|source| JailerError::Io {
                    path: stdio_log.clone(),
                    source,
                })?;
            let stderr = file.try_clone().map_err(|source| JailerError::Io {
                path: stdio_log.clone(),
                source,
            })?;
            command
                .stdout(Stdio::from(file))
                .stderr(Stdio::from(stderr));
        } else {
            command.stdout(Stdio::null()).stderr(Stdio::null());
        }

        let mut child = spawn_jailer_command(command_path, &mut command)?;

        let jailer_pid = child.id();

        let pid_file = self.jail_path.join("firecracker.pid");
        let deadline = Instant::now() + FIRECRACKER_PID_TIMEOUT;
        let Some(firecracker_pid) = wait_for_firecracker_pid_file(&pid_file, deadline)? else {
            // Timeout: firecracker.pid never appeared. Kill the child
            // (which may be jailer pre-exec or firecracker post-exec)
            // and reap it before bailing.
            let _ = child.kill();
            let _ = child.wait();
            return Err(JailerError::FirecrackerPidTimeout {
                jail_path: self.jail_path.clone(),
            });
        };

        let recorded_jailer_pid = if self.plan.config.daemonize {
            wait_for_detached_parent(&mut child, "daemonized jailer parent")?;
            0
        } else if self.plan.config.new_pid_ns {
            wait_for_detached_parent(&mut child, "new-pid-ns parent")?;
            0
        } else {
            // Do NOT wait on `child`: jailer `exec()`s into firecracker, so this
            // child handle's pid is the firecracker pid. Waiting blocks until
            // the VM exits — which we explicitly do not want here. Drop on
            // `JailedFirecracker` is responsible for kill+reap on teardown.
            std::mem::forget(child);
            jailer_pid
        };

        let state_path = self.plan.config.run_dir.join(JAILER_STATE_FILE);
        let state = JailerState {
            schema_version: 1,
            jailer_pid: Some(recorded_jailer_pid),
            firecracker_pid: Some(firecracker_pid),
        };
        let state_json = serde_json::to_vec(&state).map_err(|e| JailerError::Io {
            path: state_path.clone(),
            source: io::Error::new(io::ErrorKind::Other, e),
        })?;
        write_file_no_follow(&state_path, &state_json)?;

        Ok(JailedFirecracker {
            jailer_pid: recorded_jailer_pid,
            firecracker_pid,
        })
    }
}

fn wait_for_firecracker_pid_file(
    pid_file: &Path,
    deadline: Instant,
) -> Result<Option<u32>, JailerError> {
    wait_for_firecracker_pid_file_with_sleep(pid_file, deadline, thread::sleep)
}

fn wait_for_firecracker_pid_file_with_sleep<F>(
    pid_file: &Path,
    deadline: Instant,
    mut sleep: F,
) -> Result<Option<u32>, JailerError>
where
    F: FnMut(Duration),
{
    let mut poll_delay = FIRECRACKER_PID_INITIAL_POLL;
    loop {
        match read_firecracker_pid_file(pid_file)? {
            Some(pid) => return Ok(Some(pid)),
            None => {
                let now = Instant::now();
                if now >= deadline {
                    return Ok(None);
                }
                sleep(poll_delay.min(deadline.duration_since(now)));
                poll_delay = next_firecracker_pid_poll_delay(poll_delay);
            }
        }
    }
}

fn read_firecracker_pid_file(pid_file: &Path) -> Result<Option<u32>, JailerError> {
    let raw = match std::fs::read_to_string(pid_file) {
        Ok(raw) => raw,
        Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(JailerError::Io {
                path: pid_file.to_path_buf(),
                source,
            });
        }
    };
    let pid = raw.trim().parse().map_err(|e| JailerError::Io {
        path: pid_file.to_path_buf(),
        source: io::Error::new(
            io::ErrorKind::InvalidData,
            format!("firecracker.pid not a u32: {e}"),
        ),
    })?;
    Ok(Some(pid))
}

fn next_firecracker_pid_poll_delay(current: Duration) -> Duration {
    (current * 2).min(FIRECRACKER_PID_MAX_POLL)
}

fn push_extended_resource_limits(command: &mut Command, limits: &crate::types::ResourceLimits) {
    for (name, value) in [
        ("no-file", Some(limits.no_file)),
        ("fsize", limits.fsize),
        ("nproc", limits.nproc),
        ("memlock", limits.memlock),
        ("as", limits.address_space),
        ("core", limits.core),
        ("stack", limits.stack),
    ] {
        if let Some(value) = value {
            command.arg("--rlimit").arg(format!("{name}={value}"));
        }
    }
}

fn validate_netns_path(path: &Path) -> Result<(), JailerError> {
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_CLOEXEC)
        .open(path)
        .map_err(|source| JailerError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    let stat = nix::sys::statfs::fstatfs(&file).map_err(|source| JailerError::Io {
        path: path.to_path_buf(),
        source: io::Error::from_raw_os_error(source as i32),
    })?;
    if stat.filesystem_type() != nix::sys::statfs::NSFS_MAGIC {
        return Err(JailerError::InvalidNetns {
            path: path.to_path_buf(),
            fs_type: format!("{:?}", stat.filesystem_type()),
        });
    }
    Ok(())
}

fn wait_for_detached_parent(
    child: &mut std::process::Child,
    label: &'static str,
) -> Result<(), JailerError> {
    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        if let Some(status) = child.try_wait().map_err(|source| JailerError::Io {
            path: PathBuf::from(label),
            source,
        })? {
            if status.success() {
                return Ok(());
            }
            return Err(JailerError::Io {
                path: PathBuf::from(label),
                source: io::Error::new(
                    io::ErrorKind::Other,
                    format!("jailer parent exited with {status}"),
                ),
            });
        }

        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(JailerError::Io {
                path: PathBuf::from("jailer process"),
                source: io::Error::new(io::ErrorKind::TimedOut, format!("{label} did not exit")),
            });
        }

        thread::sleep(Duration::from_millis(25));
    }
}

fn spawn_jailer_command(command_path: &Path, command: &mut Command) -> Result<Child, JailerError> {
    let mut last_error = None;
    for attempt in 0..5 {
        match command.spawn() {
            Ok(child) => return Ok(child),
            Err(source) if source.raw_os_error() == Some(nix::libc::ETXTBSY) && attempt < 4 => {
                last_error = Some(source);
                thread::sleep(Duration::from_millis(10));
            }
            Err(source) => {
                return Err(JailerError::Io {
                    path: command_path.to_path_buf(),
                    source,
                });
            }
        }
    }

    Err(JailerError::Io {
        path: command_path.to_path_buf(),
        source: last_error.expect("ETXTBSY retry loop records the last error"),
    })
}

impl Drop for MaterializedJail {
    fn drop(&mut self) {
        use nix::mount::{umount2, MntFlags};

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
    pub(crate) jailer_pid: u32,
    /// PID of the `firecracker` child the jailer exec'd.
    pub(crate) firecracker_pid: u32,
}

impl JailedFirecracker {
    /// Construct a `JailedFirecracker` with known PIDs.
    ///
    /// Use `0` for `jailer_pid` when the jailer has already exited (daemonized mode).
    #[must_use]
    pub fn new(jailer_pid: u32, firecracker_pid: u32) -> Self {
        Self {
            jailer_pid,
            firecracker_pid,
        }
    }

    /// PID of the `jailer` process (0 when daemonized and already exited).
    #[must_use]
    pub fn jailer_pid(&self) -> u32 {
        self.jailer_pid
    }

    /// PID of the `firecracker` child the jailer exec'd.
    #[must_use]
    pub fn firecracker_pid(&self) -> u32 {
        self.firecracker_pid
    }
}

#[cfg(test)]
#[path = "materialized_tests.rs"]
mod tests;
