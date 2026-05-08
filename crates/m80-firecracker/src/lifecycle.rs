//! [`RunningSandbox`] and [`StoppedSandbox`] method implementations.
//! Transitions consume the prior handle (move semantics).

mod exec;
mod fileops;
mod health;
mod hotplug;
mod metrics;
mod protocol;
mod stopped;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use m80_observability::{ExitReason, Phase};
use m80_proto::{Envelope, ShutdownAction, ShutdownRequest, ShutdownResponse};
use m80_snapshot::{capture as snapshot_capture, CaptureRequest, SnapshotKind, SnapshotPaths};
use m80_vsock::{Channel, GUEST_PORT_DEFAULT};

use crate::diagnostics::phase_event;
use crate::error::{ConfigError, FcError, StopDisposition};
use crate::types::{RunningSandbox, StoppedSandbox};

/// Per-attempt deadline for the shutdown vsock round-trip (open UDS,
/// send request, read response).
const SHUTDOWN_RPC_TIMEOUT: Duration = Duration::from_secs(5);

const SNAPSHOT_BIND_DEST: &str = "snapshot";

fn normal_stop_disposition() -> StopDisposition {
    StopDisposition::GuestdShutdownThenFirecrackerKill
}

fn force_kill_disposition() -> StopDisposition {
    StopDisposition::HostForceKill
}

impl RunningSandbox {
    /// Return the VM id for this sandbox.
    pub fn vm_id(&self) -> &str {
        &self.vm_id
    }

    /// Capture the live VM into a snapshot pair at `paths`.
    ///
    /// Steps:
    /// 1. Pause the VM (`PATCH /vm {"state":"Paused"}`).
    /// 2. Create a Full snapshot (`PUT /snapshot/create`).
    ///
    /// **The VM is left in the Paused state after a successful call.**
    /// The caller decides the next step:
    /// - `stop()` to tear down the VM (the snapshot is archived; the VM is gone).
    /// - `resume()` (out of scope in v0.1) to continue running from the point of capture.
    ///
    /// # Note — no compile-time guard against calling `exec` after `capture`
    ///
    /// There is no type-system enforcement preventing the caller from calling
    /// `exec()` on the same `RunningSandbox` after `capture()` returns. The VM
    /// remains a `RunningSandbox` value, so the borrow checker allows it. In
    /// practice the call will fail at the Firecracker REST layer because the VM
    /// is Paused and guestd is suspended — the vsock handshake will time out or
    /// be refused. The error surface is a `FcError::Vsock` rather than a
    /// compile-time diagnostic.
    ///
    /// A future `PausedSandbox` newtype that wraps the paused VM and exposes
    /// only `stop()` (and eventually `resume()`) would close this gap at compile
    /// time. That refactor is deferred; for now, callers must treat `capture()`
    /// as terminal — either call `stop()` or `force_kill()` immediately after.
    ///
    /// # Errors
    ///
    /// Returns `FcError::Snapshot` if either REST call fails. The caller
    /// should treat any error as the VM being in an unknown state and call
    /// `force_kill()`.
    pub fn capture(&mut self, paths: SnapshotPaths) -> Result<(), FcError> {
        let snapshot_bind = bind_snapshot_parent_into_jail(
            &self.jail.jail_path,
            &paths,
            self.backend.config.jail_uid,
            self.backend.config.jail_gid,
        )?;
        let fc_socket = self.jail.jail_path.join("firecracker.sock");
        snapshot_capture(CaptureRequest {
            fc_socket,
            paths: snapshot_bind.paths.clone(),
            kind: SnapshotKind::Full,
        })
        .map_err(FcError::Snapshot)?;
        crate::diagnostics::record_stop_reason(
            &mut self.diagnostics,
            &self.vm_id,
            self.request_id.as_deref(),
            "snapshot captured",
            ExitReason::SnapshotCapture,
        );
        Ok(())
    }

    /// Five-phase teardown (`CLEANUP_PHASE_ORDER`):
    /// 1. `admission_fence` — no new exec accepted (no-op in v0.1).
    /// 2. `bounded_stop` — ask guestd to shut down over vsock, then SIGKILL
    ///    the Firecracker process after the RPC returns or fails.
    /// 3. `extract_changes` (optional) — NOT done here; caller calls
    ///    [`StoppedSandbox::extract_changes`] after receiving the `StoppedSandbox`.
    /// 4. `residue_cleanup` — remove run-root directories and scratch state.
    /// 5. `release` — foundation resources dropped, permit and scratch moved
    ///    into the returned `StoppedSandbox`.
    pub fn stop(mut self) -> Result<StoppedSandbox, FcError> {
        self.kill_guard.disarm();
        let run_root = self.backend.config.run_root.clone();
        let vsock_uds = self.jail.jail_path.join("vsock.sock");
        crate::diagnostics::record_owned(
            &mut self.diagnostics,
            Phase::Stop,
            &self.vm_id,
            self.request_id.as_deref(),
            "stop started",
        );

        // Signal the idle-watcher thread to exit before teardown so it does
        // not race with the shutdown we are about to send.
        self.watcher_stop.store(true, Ordering::Relaxed);

        // Phase 2: bounded_stop.
        let t_bounded = Instant::now();
        let exit_reason = bounded_stop(self.firecracker.firecracker_pid, &vsock_uds)?;

        // Phase 4: release. Destructure to drop everything except what moves
        // into StoppedSandbox.
        let t_release = Instant::now();
        let RunningSandbox {
            vm_id,
            request_id,
            run_dir,
            jail: _jail,     // Drop → unmounts bind mounts + removes jail dir.
            cgroup: _cgroup, // Drop → removes cgroup subtree.
            rootfs: _rootfs,
            scratch,
            snapshot_mount,
            client: _client,
            firecracker: _firecracker,
            permit,
            backend: _backend,
            last_activity_ns: _last_activity_ns,
            active_execs: _active_execs,
            idle_timed_out: _idle_timed_out,
            watcher_stop: _watcher_stop,
            watcher_thread,
            diagnostics,
            preallocated_drive_slots: _preallocated_drive_slots,
            one_shot: _one_shot,
            one_shot_consumed: _one_shot_consumed,
            kill_guard: _kill_guard,
        } = self;
        let mut diagnostics = diagnostics;

        phase_event("stop_bounded", &vm_id, t_bounded.elapsed());
        // Join the watcher thread after destructuring (the stop flag is already
        // set above; the thread will exit on its next wake interval).
        if let Some(handle) = watcher_thread {
            let _ = handle.join();
        }
        unmount_snapshot_bind(snapshot_mount.as_deref());
        phase_event("stop_release", &vm_id, t_release.elapsed());
        crate::diagnostics::record_stop_reason(
            &mut diagnostics,
            &vm_id,
            request_id.as_deref(),
            "stop complete",
            exit_reason,
        );

        Ok(StoppedSandbox {
            vm_id,
            request_id,
            run_dir,
            scratch,
            permit,
            run_root,
            diagnostics,
        })
    }

    /// Last-resort: SIGKILL the firecracker and jailer pids immediately.
    ///
    /// The run-dir is preserved for offline inspection. The caller decides
    /// whether to delete it via [`StoppedSandbox::delete`] or preserve it
    /// further via [`StoppedSandbox::preserve_for_triage`].
    pub fn force_kill(mut self) -> Result<StoppedSandbox, FcError> {
        self.kill_guard.disarm();
        let run_root = self.backend.config.run_root.clone();
        crate::diagnostics::record_owned(
            &mut self.diagnostics,
            Phase::Stop,
            &self.vm_id,
            self.request_id.as_deref(),
            "force kill started",
        );

        // Signal the watcher to exit before killing the process.
        self.watcher_stop.store(true, Ordering::Relaxed);

        match force_kill_disposition() {
            StopDisposition::HostForceKill => {
                kill_pid(self.firecracker.firecracker_pid)?;
                kill_pid(self.firecracker.jailer_pid)?;
            }
            StopDisposition::GuestdShutdownThenFirecrackerKill => {
                unreachable!("force kill disposition")
            }
        }

        let RunningSandbox {
            vm_id,
            request_id,
            run_dir,
            jail: _jail,
            cgroup: _cgroup,
            rootfs: _rootfs,
            scratch,
            snapshot_mount,
            client: _client,
            firecracker: _firecracker,
            permit,
            backend: _backend,
            last_activity_ns: _last_activity_ns,
            active_execs: _active_execs,
            idle_timed_out: _idle_timed_out,
            watcher_stop: _watcher_stop,
            watcher_thread,
            diagnostics,
            preallocated_drive_slots: _preallocated_drive_slots,
            one_shot: _one_shot,
            one_shot_consumed: _one_shot_consumed,
            kill_guard: _kill_guard,
        } = self;
        let mut diagnostics = diagnostics;

        if let Some(handle) = watcher_thread {
            let _ = handle.join();
        }
        unmount_snapshot_bind(snapshot_mount.as_deref());
        crate::diagnostics::record_stop_reason(
            &mut diagnostics,
            &vm_id,
            request_id.as_deref(),
            "force kill complete",
            ExitReason::ForceKill,
        );

        Ok(StoppedSandbox {
            vm_id,
            request_id,
            run_dir,
            scratch,
            permit,
            run_root,
            diagnostics,
        })
    }
}

pub(crate) struct SnapshotBind {
    pub(crate) paths: SnapshotPaths,
    mount_path: PathBuf,
    active: bool,
}

impl SnapshotBind {
    pub(crate) fn into_mount_path(mut self) -> PathBuf {
        self.active = false;
        self.mount_path.clone()
    }
}

impl Drop for SnapshotBind {
    fn drop(&mut self) {
        if self.active {
            unmount_snapshot_bind(Some(&self.mount_path));
        }
    }
}

pub(crate) fn bind_snapshot_parent_into_jail(
    jail_path: &Path,
    paths: &SnapshotPaths,
    jail_uid: u32,
    jail_gid: u32,
) -> Result<SnapshotBind, FcError> {
    let host_parent = paths.vm_state.parent().ok_or_else(|| {
        FcError::Config(ConfigError::InvalidValue {
            field: "snapshot.vm_state",
            reason: "path must have a parent directory".into(),
        })
    })?;
    let mem_parent = paths.mem.parent().ok_or_else(|| {
        FcError::Config(ConfigError::InvalidValue {
            field: "snapshot.mem",
            reason: "path must have a parent directory".into(),
        })
    })?;
    if host_parent != mem_parent {
        return Err(FcError::Config(ConfigError::InvalidValue {
            field: "snapshot",
            reason: "vm_state and mem paths must live in the same directory".into(),
        }));
    }
    let vm_name = paths.vm_state.file_name().ok_or_else(|| {
        FcError::Config(ConfigError::InvalidValue {
            field: "snapshot.vm_state",
            reason: "path must have a file name".into(),
        })
    })?;
    let mem_name = paths.mem.file_name().ok_or_else(|| {
        FcError::Config(ConfigError::InvalidValue {
            field: "snapshot.mem",
            reason: "path must have a file name".into(),
        })
    })?;

    std::fs::create_dir_all(host_parent)?;
    let mount_path = jail_path.join(SNAPSHOT_BIND_DEST);
    std::fs::create_dir_all(&mount_path)?;

    use nix::mount::{mount, MsFlags};
    use nix::unistd::{chown, Gid, Uid};

    chown(
        host_parent,
        Some(Uid::from_raw(jail_uid)),
        Some(Gid::from_raw(jail_gid)),
    )
    .map_err(|e| FcError::Io(std::io::Error::from_raw_os_error(e as i32)))?;

    mount(
        Some(host_parent),
        mount_path.as_path(),
        None::<&str>,
        MsFlags::MS_BIND,
        None::<&str>,
    )
    .map_err(|e| FcError::Io(std::io::Error::from_raw_os_error(e as i32)))?;

    let in_jail_parent = PathBuf::from("/").join(SNAPSHOT_BIND_DEST);
    Ok(SnapshotBind {
        paths: SnapshotPaths {
            vm_state: in_jail_parent.join(vm_name),
            mem: in_jail_parent.join(mem_name),
        },
        mount_path,
        active: true,
    })
}

pub(crate) fn unmount_snapshot_bind(mount_path: Option<&Path>) {
    let Some(mount_path) = mount_path else {
        return;
    };
    use nix::mount::{umount2, MntFlags};
    if let Err(e) = umount2(mount_path, MntFlags::MNT_DETACH) {
        tracing::warn!(path = %mount_path.display(), err = %e, "snapshot bind unmount failed");
    }
    if let Err(e) = std::fs::remove_dir(mount_path) {
        tracing::warn!(path = %mount_path.display(), err = %e, "snapshot bind mount dir cleanup failed");
    }
}

/// Graceful stop: send `ShutdownRequest` over vsock, then immediately
/// SIGKILL the Firecracker process.
///
/// The ack from the guest proves filesystems are synced and the guest is
/// done. There's nothing useful left to wait for: a clean kernel exit
/// produces the same observable post-state as a SIGKILL'd Firecracker
/// from the host's perspective (cgroup teardown, run-dir cleanup all
/// happen in `RunningSandbox::stop`'s post-bounded-stop release phase).
/// Waiting for kernel reboot was burning ~1 s per launch on minimal kind
/// (`panic=1` reboot delay) and ~half that on ubuntu.
///
/// If the vsock RPC fails (guest unreachable / already dead), SIGKILL
/// directly — same outcome.
fn bounded_stop(firecracker_pid: u32, vsock_uds: &Path) -> Result<ExitReason, FcError> {
    match normal_stop_disposition() {
        StopDisposition::GuestdShutdownThenFirecrackerKill => {
            let exit_reason = if let Err(e) = send_shutdown_request(vsock_uds) {
                tracing::warn!(
                    error = %e,
                    "vsock graceful-stop failed; SIGKILLing without ack"
                );
                ExitReason::ForceKill
            } else {
                ExitReason::NormalStop
            };
            kill_pid(firecracker_pid)?;
            Ok(exit_reason)
        }
        StopDisposition::HostForceKill => unreachable!("normal stop disposition"),
    }
}

/// Open a fresh vsock channel, send `ShutdownRequest`, read
/// `ShutdownResponse`. Returns the guest's chosen action.
fn send_shutdown_request(vsock_uds: &Path) -> Result<ShutdownAction, FcError> {
    let deadline = Instant::now() + SHUTDOWN_RPC_TIMEOUT;
    let mut channel = Channel::open_uds_only(vsock_uds, GUEST_PORT_DEFAULT)?;

    let req = ShutdownRequest { reason: None };
    channel.send(&Envelope::new(req))?;

    let resp_env: Envelope<ShutdownResponse> = channel.recv()?;
    if Instant::now() > deadline {
        return Err(FcError::Vsock(m80_vsock::VsockError::NotReady));
    }
    Ok(resp_env.payload.action)
}

/// Return a monotonic timestamp in nanoseconds.
///
/// Uses `Instant` internally; the absolute value is arbitrary but consistent
/// within a process lifetime. The watcher and exec share a common epoch
/// because they both call this function.
pub(crate) fn monotonic_ns() -> u64 {
    // Instant cannot be stored in an AtomicU64 directly; we compute the
    // duration from a fixed reference point instead.
    use std::sync::OnceLock;
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    let epoch = EPOCH.get_or_init(Instant::now);
    epoch.elapsed().as_nanos() as u64
}

/// Spawn the idle-timeout watcher thread.
///
/// The watcher sleeps in a loop, waking every `poll_interval` to compare
/// the elapsed time since `last_activity_ns` against `timeout`. In-flight execs
/// suppress the idle decision; their drop guard resets the deadline when the
/// request completes. When the deadline expires, the watcher sends a graceful
/// shutdown request over `vsock_uds` (best-effort; logs on failure) and sets
/// `idle_timed_out` so the next `exec` returns `FcError::IdleTimedOut`.
///
/// The thread exits when `stop_flag` is set (by `stop()` or `force_kill()`).
pub(crate) fn spawn_idle_watcher(
    timeout: Duration,
    vsock_uds: std::path::PathBuf,
    last_activity_ns: Arc<AtomicU64>,
    active_execs: Arc<AtomicUsize>,
    idle_timed_out: Arc<AtomicBool>,
    stop_flag: Arc<AtomicBool>,
    vm_id: String,
) -> std::thread::JoinHandle<()> {
    // Poll at most at this interval. Chosen to be short enough to be
    // responsive but not so short it wastes CPU.
    let poll_interval = std::cmp::min(timeout / 4, Duration::from_secs(30));
    std::thread::spawn(move || {
        idle_watcher_loop(
            timeout,
            poll_interval,
            IdleWatcherContext {
                vsock_uds: &vsock_uds,
                last_activity_ns: &last_activity_ns,
                active_execs: &active_execs,
                idle_timed_out: &idle_timed_out,
                stop_flag: &stop_flag,
                vm_id: &vm_id,
            },
        );
    })
}

pub(crate) struct IdleWatcherContext<'a> {
    pub(crate) vsock_uds: &'a std::path::Path,
    pub(crate) last_activity_ns: &'a AtomicU64,
    pub(crate) active_execs: &'a AtomicUsize,
    pub(crate) idle_timed_out: &'a AtomicBool,
    pub(crate) stop_flag: &'a AtomicBool,
    pub(crate) vm_id: &'a str,
}

/// Inner loop of the idle-timeout watcher. Extracted so it's testable
/// without spawning a real thread.
pub(crate) fn idle_watcher_loop(
    timeout: Duration,
    poll_interval: Duration,
    context: IdleWatcherContext<'_>,
) {
    loop {
        std::thread::sleep(poll_interval);

        if context.stop_flag.load(Ordering::Relaxed) {
            return;
        }

        if context.active_execs.load(Ordering::Relaxed) > 0 {
            continue;
        }

        let last_ns = context.last_activity_ns.load(Ordering::Relaxed);
        let now_ns = monotonic_ns();
        let idle_ns = now_ns.saturating_sub(last_ns);
        let timeout_ns = timeout.as_nanos() as u64;

        if idle_ns >= timeout_ns {
            tracing::info!(
                vm_id = context.vm_id,
                "idle timeout expired; issuing graceful shutdown"
            );
            context.idle_timed_out.store(true, Ordering::Relaxed);
            if let Err(e) = send_shutdown_request(context.vsock_uds) {
                tracing::warn!(
                    vm_id = context.vm_id,
                    error = %e,
                    "idle watcher: graceful shutdown failed (VM may already be stopped)"
                );
            }
            return;
        }
    }
}

/// Send `SIGKILL` to `pid`. Treats `ESRCH` (no such process) as success.
pub(crate) fn kill_pid(pid: u32) -> Result<(), FcError> {
    use nix::errno::Errno;
    use nix::sys::signal::{kill, Signal};
    use nix::unistd::Pid;

    if pid == 0 {
        return Ok(());
    }

    match kill(Pid::from_raw(pid as i32), Signal::SIGKILL) {
        Ok(()) => Ok(()),
        Err(Errno::ESRCH) => Ok(()), // Process already gone.
        Err(e) => Err(FcError::Io(std::io::Error::from_raw_os_error(e as i32))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    #[test]
    fn normal_stop_is_arch_independent_guestd_shutdown_then_firecracker_kill() {
        assert_eq!(
            normal_stop_disposition(),
            StopDisposition::GuestdShutdownThenFirecrackerKill
        );
    }

    #[test]
    fn force_kill_targets_firecracker_and_jailer_without_guest_rpc() {
        assert_eq!(force_kill_disposition(), StopDisposition::HostForceKill);
    }

    #[test]
    fn kill_pid_zero_is_no_live_jailer_sentinel() {
        kill_pid(0).expect("pid zero sentinel is a no-op");
    }

    #[test]
    fn idle_watcher_does_not_fire_while_exec_is_in_flight() {
        let last_activity = AtomicU64::new(0);
        let active_execs = AtomicUsize::new(1);
        let idle_timed_out = AtomicBool::new(false);
        let stop_flag = AtomicBool::new(false);
        let socket = std::path::PathBuf::from("/tmp/m80-idle-watcher-test.sock");

        std::thread::scope(|scope| {
            let handle = scope.spawn(|| {
                idle_watcher_loop(
                    Duration::from_millis(20),
                    Duration::from_millis(5),
                    IdleWatcherContext {
                        vsock_uds: &socket,
                        last_activity_ns: &last_activity,
                        active_execs: &active_execs,
                        idle_timed_out: &idle_timed_out,
                        stop_flag: &stop_flag,
                        vm_id: "vm-test",
                    },
                );
            });
            std::thread::sleep(Duration::from_millis(40));
            stop_flag.store(true, Ordering::Relaxed);
            handle.join().expect("watcher exits after stop");
        });

        assert!(!idle_timed_out.load(Ordering::Relaxed));
    }
}
