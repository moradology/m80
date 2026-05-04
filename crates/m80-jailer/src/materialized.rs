//! `MaterializedJail` (handle returned by `Plan::materialize`),
//! `MaterializedJail::launch` (jailer exec), `Drop` (chroot teardown),
//! and `JailedFirecracker` (live pids).

use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use tracing::warn;

use crate::error::JailerError;
use crate::types::{JailerState, Plan, JAILER_STATE_FILE};

/// A materialized chroot. Drop tears it down.
#[derive(Debug)]
pub struct MaterializedJail {
    /// The plan that produced this jail.
    pub plan: Plan,
    /// Path to the chroot root.
    pub jail_path: PathBuf,
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
        // (matches `chroot_path()` in `types.rs`).
        let chroot_base = self.plan.config.run_dir.as_path();

        // Jailer args end at the bare `--`; everything after is forwarded
        // to the jailed firecracker binary. `--api-sock` is firecracker's
        // arg, not jailer's, so it goes on the right side of the separator.
        //
        // We do NOT pass `--daemonize`. Without it, jailer `exec()`s into
        // firecracker, so this `Child` handle's pid IS the firecracker pid.
        // We never `wait()` on it (that would block until the VM exits).
        // Drop on `JailedFirecracker` is responsible for kill+reap.
        //
        // Stdout/stderr inherit from m80; firecracker prints VMM logs and
        // (if `console=ttyS0` is in the boot args) serial console output
        // there too — invaluable for boot debugging. With `--daemonize`,
        // those would be redirected to /dev/null.
        let mut child = Command::new(&self.plan.config.jailer_bin)
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
            .arg("--")
            .arg("--api-sock")
            .arg(api_socket_name)
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
                let raw = std::fs::read_to_string(&pid_file).map_err(|source| JailerError::Io {
                    path: pid_file.clone(),
                    source,
                })?;
                let pid: u32 = raw.trim().parse().map_err(|e| JailerError::Io {
                    path: pid_file.clone(),
                    source: io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("firecracker.pid not a u32: {e}"),
                    ),
                })?;
                break pid;
            }
            if Instant::now() >= deadline {
                // Timeout: firecracker.pid never appeared. Kill the child
                // (which may be jailer pre-exec or firecracker post-exec)
                // and reap it before bailing.
                let _ = child.kill();
                let _ = child.wait();
                return Err(JailerError::ChrootFailed {
                    jail_path: self.jail_path.clone(),
                });
            }
            thread::sleep(Duration::from_millis(25));
        };

        // Do NOT wait on `child`: jailer `exec()`s into firecracker, so this
        // child handle's pid is the firecracker pid. Waiting blocks until
        // the VM exits — which we explicitly do not want here. Drop on
        // `JailedFirecracker` is responsible for kill+reap on teardown.
        std::mem::forget(child);

        // Persist updated state.
        let state_path = self.plan.config.run_dir.join(JAILER_STATE_FILE);
        let state = JailerState {
            jailer_pid: Some(jailer_pid),
            firecracker_pid: Some(firecracker_pid),
        };
        let state_json = serde_json::to_vec_pretty(&state).map_err(|e| JailerError::Io {
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
        use nix::mount::{umount2, MntFlags};

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
