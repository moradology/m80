//! `MaterializedJail` (handle returned by `Plan::materialize`),
//! `MaterializedJail::launch` (jailer exec), `Drop` (chroot teardown),
//! and `JailedFirecracker` (live pids).

use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
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
        // (matches `jail_root_path()` in `types.rs`).
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
        let mut command = Command::new(&self.plan.config.jailer_bin);
        command
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
            .arg(api_socket_name);

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
        }

        let mut child = command.spawn().map_err(|source| JailerError::Io {
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
                return Err(JailerError::FirecrackerPidTimeout {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{JailerConfig, Plan};
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn launch_redirects_stdio_to_configured_log() {
        let dir = tempfile::tempdir().unwrap();
        let run_dir = dir.path().join("vm-stdio");
        std::fs::create_dir_all(&run_dir).unwrap();
        let jailer_bin = dir.path().join("fake-jailer.sh");
        std::fs::write(
            &jailer_bin,
            r#"#!/bin/sh
id=
chroot_base=
exec_file=
while [ "$#" -gt 0 ]; do
  case "$1" in
    --id) id="$2"; shift 2 ;;
    --chroot-base-dir) chroot_base="$2"; shift 2 ;;
    --exec-file) exec_file="$2"; shift 2 ;;
    --) shift; break ;;
    *) shift ;;
  esac
done
exec_base=$(basename "$exec_file")
jail_root="$chroot_base/$exec_base/$id/root"
mkdir -p "$jail_root"
echo $$ > "$jail_root/firecracker.pid"
echo fake-firecracker-stdout
echo fake-firecracker-stderr >&2
sleep 30
"#,
        )
        .unwrap();
        let mut perms = std::fs::metadata(&jailer_bin).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&jailer_bin, perms).unwrap();

        let stdio_log = run_dir.join("console.log");
        let cfg = JailerConfig {
            jailer_bin,
            firecracker_bin: PathBuf::from("/usr/bin/firecracker"),
            run_dir: run_dir.clone(),
            uid: 3000,
            gid: 3000,
            bindings: Vec::new(),
            sockets: Vec::new(),
            stdio_log: Some(stdio_log.clone()),
        };
        let plan = Plan::compute(&cfg).unwrap();
        let jail_path = run_dir.join("firecracker").join("vm-stdio").join("root");
        let jail = MaterializedJail {
            plan,
            jail_path,
            bind_mounts: Vec::new(),
            created_dirs: Vec::new(),
            placeholder_files: Vec::new(),
        };

        let jailed = jail.launch(Path::new("firecracker.sock")).unwrap();
        nix::sys::signal::kill(
            nix::unistd::Pid::from_raw(jailed.jailer_pid as i32),
            nix::sys::signal::Signal::SIGKILL,
        )
        .unwrap();

        let log = std::fs::read_to_string(stdio_log).unwrap();
        assert!(log.contains("fake-firecracker-stdout"), "{log}");
        assert!(log.contains("fake-firecracker-stderr"), "{log}");
    }
}
