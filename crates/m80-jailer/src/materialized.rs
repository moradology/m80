//! `MaterializedJail` (handle returned by `Plan::materialize`),
//! `MaterializedJail::launch` (jailer exec), `Drop` (chroot teardown),
//! and `JailedFirecracker` (live pids).

use std::io;
use std::os::unix::fs::OpenOptionsExt;
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
            if let Some(harden_bin) = &self.plan.config.jailer_harden_bin {
                let mut command = Command::new(harden_bin);
                command
                    .arg("--jailer-bin")
                    .arg(&self.plan.config.jailer_bin)
                    .arg("--uid")
                    .arg(self.plan.config.uid.to_string())
                    .arg("--gid")
                    .arg(self.plan.config.gid.to_string())
                    .arg("--");
                (harden_bin, command)
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

        let mut child = command.spawn().map_err(|source| JailerError::Io {
            path: command_path.to_path_buf(),
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

        // Persist updated state.
        let state_path = self.plan.config.run_dir.join(JAILER_STATE_FILE);
        let state = JailerState {
            jailer_pid: Some(recorded_jailer_pid),
            firecracker_pid: Some(firecracker_pid),
        };
        let state_json = serde_json::to_vec_pretty(&state).map_err(|e| JailerError::Io {
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

fn write_file_no_follow(path: &Path, bytes: &[u8]) -> Result<(), JailerError> {
    use std::io::Write;

    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .custom_flags(nix::libc::O_NOFOLLOW)
        .open(path)
        .map_err(|source| JailerError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    file.write_all(bytes).map_err(|source| JailerError::Io {
        path: path.to_path_buf(),
        source,
    })
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

    struct EnvGuard {
        key: &'static str,
        previous: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let previous = std::env::var(key).ok();
            std::env::set_var(key, value);
            Self { key, previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            if let Some(previous) = &self.previous {
                std::env::set_var(self.key, previous);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }

    #[test]
    fn launch_redirects_stdio_and_passes_hardening_args() {
        let dir = tempfile::tempdir().unwrap();
        let run_dir = dir.path().join("vm-stdio");
        std::fs::create_dir_all(&run_dir).unwrap();
        let jailer_bin = dir.path().join("fake-jailer.sh");
        let harden_bin = dir.path().join("fake-harden.sh");
        let harden_args_path = run_dir.join("harden-args.txt");
        std::fs::write(
            &harden_bin,
            format!(
                r#"#!/bin/sh
printf '%s\n' "$*" > "{harden_args}"
jailer=
while [ "$#" -gt 0 ]; do
  case "$1" in
    --jailer-bin) jailer="$2"; shift 2 ;;
    --uid) shift 2 ;;
    --gid) shift 2 ;;
    --) shift; break ;;
    *) exit 64 ;;
  esac
done
exec "$jailer" "$@"
"#,
                harden_args = harden_args_path.display()
            ),
        )
        .unwrap();
        std::fs::write(
            &jailer_bin,
            r#"#!/bin/sh
if [ -n "$M80_JAILER_ENV_LEAK" ]; then
  echo env-leaked >&2
  exit 44
fi
id=
chroot_base=
exec_file=
all_args="$*"
while [ "$#" -gt 0 ]; do
  case "$1" in
    --id) id="$2"; shift 2 ;;
    --chroot-base-dir) chroot_base="$2"; shift 2 ;;
    --exec-file) exec_file="$2"; shift 2 ;;
    --) shift; break ;;
    *) shift ;;
  esac
done
exec_base="${exec_file##*/}"
jail_root="$chroot_base/$exec_base/$id/root"
/bin/mkdir -p "$jail_root"
printf '%s\n' "$all_args" > "$chroot_base/args.txt"
echo $$ > "$jail_root/firecracker.pid"
echo fake-firecracker-stdout
echo fake-firecracker-stderr >&2
/bin/sleep 30
"#,
        )
        .unwrap();
        let mut perms = std::fs::metadata(&jailer_bin).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&jailer_bin, perms).unwrap();
        let mut perms = std::fs::metadata(&harden_bin).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&harden_bin, perms).unwrap();

        let stdio_log = run_dir.join("console.log");
        let cfg = JailerConfig {
            jailer_bin,
            jailer_harden_bin: Some(harden_bin),
            firecracker_bin: PathBuf::from("/usr/bin/firecracker"),
            run_dir: run_dir.clone(),
            uid: 3000,
            gid: 3000,
            bindings: Vec::new(),
            sockets: Vec::new(),
            resource_limits: crate::types::ResourceLimits {
                no_file: 1024,
                fsize: Some(4096),
            },
            new_pid_ns: false,
            daemonize: false,
            netns_path: None,
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

        let _env_guard = EnvGuard::set("M80_JAILER_ENV_LEAK", "secret");
        let jailed = jail.launch(Path::new("firecracker.sock")).unwrap();
        nix::sys::signal::kill(
            nix::unistd::Pid::from_raw(jailed.jailer_pid as i32),
            nix::sys::signal::Signal::SIGKILL,
        )
        .unwrap();

        let log = std::fs::read_to_string(stdio_log).unwrap();
        assert!(log.contains("fake-firecracker-stdout"), "{log}");
        assert!(log.contains("fake-firecracker-stderr"), "{log}");

        let args = std::fs::read_to_string(run_dir.join("args.txt")).unwrap();
        assert!(args.contains("--resource-limit no-file=1024"), "{args}");
        assert!(args.contains("--resource-limit fsize=4096"), "{args}");
        let harden_args = std::fs::read_to_string(harden_args_path).unwrap();
        assert!(harden_args.contains("--jailer-bin"), "{harden_args}");
        assert!(harden_args.contains("--uid 3000"), "{harden_args}");
        assert!(harden_args.contains("--gid 3000"), "{harden_args}");
    }

    #[test]
    fn launch_with_new_pid_ns_reaps_jailer_parent() {
        let dir = tempfile::tempdir().unwrap();
        let run_dir = dir.path().join("vm-newpid");
        std::fs::create_dir_all(&run_dir).unwrap();
        let jailer_bin = dir.path().join("fake-jailer-newpid.sh");
        std::fs::write(
            &jailer_bin,
            r#"#!/bin/sh
id=
chroot_base=
exec_file=
new_pid_ns=0
while [ "$#" -gt 0 ]; do
  case "$1" in
    --id) id="$2"; shift 2 ;;
    --chroot-base-dir) chroot_base="$2"; shift 2 ;;
    --exec-file) exec_file="$2"; shift 2 ;;
    --new-pid-ns) new_pid_ns=1; shift ;;
    --) shift; break ;;
    *) shift ;;
  esac
done
exec_base="${exec_file##*/}"
jail_root="$chroot_base/$exec_base/$id/root"
/bin/mkdir -p "$jail_root"
echo $$ > "$jail_root/firecracker.pid"
if [ "$new_pid_ns" -eq 1 ]; then
  exit 0
fi
/bin/sleep 30
"#,
        )
        .unwrap();
        let mut perms = std::fs::metadata(&jailer_bin).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&jailer_bin, perms).unwrap();

        let cfg = JailerConfig {
            jailer_bin,
            jailer_harden_bin: None,
            firecracker_bin: PathBuf::from("/usr/bin/firecracker"),
            run_dir: run_dir.clone(),
            uid: 3000,
            gid: 3000,
            bindings: Vec::new(),
            sockets: Vec::new(),
            resource_limits: crate::types::ResourceLimits::default(),
            new_pid_ns: true,
            daemonize: false,
            netns_path: None,
            stdio_log: None,
        };
        let plan = Plan::compute(&cfg).unwrap();
        let jail_path = run_dir.join("firecracker").join("vm-newpid").join("root");
        let jail = MaterializedJail {
            plan,
            jail_path,
            bind_mounts: Vec::new(),
            created_dirs: Vec::new(),
            placeholder_files: Vec::new(),
        };

        let jailed = jail.launch(Path::new("firecracker.sock")).unwrap();
        assert_eq!(jailed.jailer_pid, 0);

        let state = std::fs::read_to_string(run_dir.join("jailer-state.json")).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&state).unwrap();
        assert_eq!(parsed["jailer_pid"], 0);
    }

    #[test]
    fn launch_with_daemonize_reaps_jailer_parent_and_records_daemon_pid() {
        let dir = tempfile::tempdir().unwrap();
        let run_dir = dir.path().join("vm-daemon");
        std::fs::create_dir_all(&run_dir).unwrap();
        let jailer_bin = dir.path().join("fake-jailer-daemon.sh");
        std::fs::write(
            &jailer_bin,
            r#"#!/bin/sh
id=
chroot_base=
exec_file=
daemonize=0
while [ "$#" -gt 0 ]; do
  case "$1" in
    --id) id="$2"; shift 2 ;;
    --chroot-base-dir) chroot_base="$2"; shift 2 ;;
    --exec-file) exec_file="$2"; shift 2 ;;
    --daemonize) daemonize=1; shift ;;
    --) shift; break ;;
    *) shift ;;
  esac
done
exec_base="${exec_file##*/}"
jail_root="$chroot_base/$exec_base/$id/root"
/bin/mkdir -p "$jail_root"
if [ "$daemonize" -eq 1 ]; then
  /bin/sleep 30 &
  echo $! > "$jail_root/firecracker.pid"
  exit 0
fi
echo $$ > "$jail_root/firecracker.pid"
/bin/sleep 30
"#,
        )
        .unwrap();
        let mut perms = std::fs::metadata(&jailer_bin).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&jailer_bin, perms).unwrap();

        let cfg = JailerConfig {
            jailer_bin,
            jailer_harden_bin: None,
            firecracker_bin: PathBuf::from("/usr/bin/firecracker"),
            run_dir: run_dir.clone(),
            uid: 3000,
            gid: 3000,
            bindings: Vec::new(),
            sockets: Vec::new(),
            resource_limits: crate::types::ResourceLimits::default(),
            new_pid_ns: false,
            daemonize: true,
            netns_path: None,
            stdio_log: None,
        };
        let plan = Plan::compute(&cfg).unwrap();
        let jail_path = run_dir.join("firecracker").join("vm-daemon").join("root");
        let jail = MaterializedJail {
            plan,
            jail_path,
            bind_mounts: Vec::new(),
            created_dirs: Vec::new(),
            placeholder_files: Vec::new(),
        };

        let jailed = jail.launch(Path::new("firecracker.sock")).unwrap();
        assert_eq!(jailed.jailer_pid, 0);
        assert!(
            std::path::Path::new(&format!("/proc/{}", jailed.firecracker_pid)).exists(),
            "daemonized firecracker pid must remain live after jailer parent exits"
        );

        let state = std::fs::read_to_string(run_dir.join("jailer-state.json")).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&state).unwrap();
        assert_eq!(parsed["jailer_pid"], 0);
        assert_eq!(parsed["firecracker_pid"], jailed.firecracker_pid);

        nix::sys::signal::kill(
            nix::unistd::Pid::from_raw(jailed.firecracker_pid as i32),
            nix::sys::signal::Signal::SIGKILL,
        )
        .unwrap();
    }

    #[test]
    fn launch_without_stdio_log_uses_dev_null_stdio() {
        let dir = tempfile::tempdir().unwrap();
        let run_dir = dir.path().join("vm-null-stdio");
        std::fs::create_dir_all(&run_dir).unwrap();
        let jailer_bin = dir.path().join("fake-jailer-null-stdio.sh");
        let marker = run_dir.join("stdio.txt");
        std::fs::write(
            &jailer_bin,
            format!(
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
exec_base="${{exec_file##*/}}"
jail_root="$chroot_base/$exec_base/$id/root"
/bin/mkdir -p "$jail_root"
stdio="$(readlink /proc/$$/fd/0) $(readlink /proc/$$/fd/1) $(readlink /proc/$$/fd/2)"
printf '%s\n' "$stdio" > "{marker}"
echo $$ > "$jail_root/firecracker.pid"
/bin/sleep 30
"#,
                marker = marker.display()
            ),
        )
        .unwrap();
        let mut perms = std::fs::metadata(&jailer_bin).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&jailer_bin, perms).unwrap();

        let cfg = JailerConfig {
            jailer_bin,
            jailer_harden_bin: None,
            firecracker_bin: PathBuf::from("/usr/bin/firecracker"),
            run_dir: run_dir.clone(),
            uid: 3000,
            gid: 3000,
            bindings: Vec::new(),
            sockets: Vec::new(),
            resource_limits: crate::types::ResourceLimits::default(),
            new_pid_ns: false,
            daemonize: false,
            netns_path: None,
            stdio_log: None,
        };
        let plan = Plan::compute(&cfg).unwrap();
        let jail_path = run_dir
            .join("firecracker")
            .join("vm-null-stdio")
            .join("root");
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

        let stdio = std::fs::read_to_string(marker).unwrap();
        assert_eq!(
            stdio.split_whitespace().collect::<Vec<_>>(),
            vec!["/dev/null", "/dev/null", "/dev/null"]
        );
    }

    #[test]
    fn validate_netns_path_rejects_regular_file() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let err = validate_netns_path(file.path()).unwrap_err();
        assert!(
            matches!(err, JailerError::InvalidNetns { .. }),
            "expected InvalidNetns for regular file, got {err:?}"
        );
    }

    #[test]
    fn validate_netns_path_rejects_symlink() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target");
        let link = dir.path().join("link");
        std::fs::write(&target, b"not a namespace").unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let err = validate_netns_path(&link).unwrap_err();
        assert!(
            matches!(err, JailerError::Io { .. }),
            "expected O_NOFOLLOW open failure for symlink, got {err:?}"
        );
    }
}
