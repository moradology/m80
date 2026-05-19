//! [`RunningSandbox`] and [`StoppedSandbox`] method implementations.
//! Transitions consume the prior handle (move semantics).

mod exec;
mod fileops;
mod health;
mod hotplug;
mod metrics;
mod pmem;
mod post_restore;
mod protocol;
mod stopped;
mod template_snapshot;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use m80_observability::{ExitReason, Phase};
use m80_proto::GUEST_PORT_DEFAULT;
use m80_proto::{Envelope, ShutdownAction, ShutdownRequest, ShutdownResponse};
use m80_snapshot::{
    capture as snapshot_capture, write_snapshot_manifest, CaptureRequest, SnapshotKind,
    SnapshotPaths, SNAPSHOT_MANIFEST_FILE,
};
use m80_vsock::Channel;

use crate::diagnostics::phase_event;
use crate::error::{
    CleanupReleaseBlocker, ConfigError, FcError, StopDisposition, WireProtocolError,
};
use crate::layout::{FIRECRACKER_API_SOCKET, VSOCK_SOCKET};
use crate::types::{RunningSandbox, StoppedSandbox};

pub(crate) use pmem::phase_13_pmem_guest_mount;
pub(crate) use post_restore::phase_restore_post_restore_hooks;
pub(crate) use template_snapshot::prepare_template_snapshot_paths;

/// Per-attempt deadline for the shutdown vsock round-trip (open UDS, send
/// request, read response). 5 s is generous relative to the sub-100 ms
/// empirical round-trip; the headroom covers a loaded host where the guest may
/// need a moment to drain pending I/O before responding to the shutdown frame.
/// If the timeout fires the caller falls back to a force-kill.
const SHUTDOWN_RPC_TIMEOUT: Duration = Duration::from_secs(5);
const REAP_TIMEOUT: Duration = Duration::from_secs(2);
const REAP_INITIAL_POLL_DELAY: Duration = Duration::from_millis(1);
const REAP_MAX_POLL_DELAY: Duration = Duration::from_millis(20);
#[cfg(debug_assertions)]
const FORCE_KILL_EPERM_FOR_PID_ENV: &str = "M80_TEST_FORCE_KILL_EPERM_FOR_PID";

pub(crate) const SNAPSHOT_BIND_DEST: &str = "snapshot";

fn normal_stop_disposition() -> StopDisposition {
    StopDisposition::GuestdShutdownThenFirecrackerKill
}

fn force_kill_disposition() -> StopDisposition {
    StopDisposition::HostForceKill
}

fn release_shared_pmem_refs(refs: Vec<m80_image_store::SharedImageRef>) -> Result<(), FcError> {
    for shared_ref in refs {
        shared_ref.release().map_err(FcError::ImageStore)?;
    }
    Ok(())
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
        let snapshot_plan = prepare_snapshot_paths(&paths, &self.backend.config.run_root, true)?;
        let stage_paths = snapshot_plan.stage_paths(&self.run_dir);
        clean_snapshot_stage_files(&stage_paths)?;
        let api_socket = self.jail.jail_root().join(FIRECRACKER_API_SOCKET);
        let expected_firecracker_version = self
            .backend
            .config
            .discovery
            .manifest
            .expected_firecracker_version
            .clone();
        snapshot_capture(CaptureRequest {
            api_socket,
            paths: snapshot_plan.jail_paths,
            host_paths: stage_paths.clone(),
            expected_firecracker_version: expected_firecracker_version.clone(),
            kind: SnapshotKind::Full,
        })
        .map_err(FcError::Snapshot)?;
        move_captured_snapshot_from_stage(&stage_paths, &paths)?;
        write_snapshot_manifest(&paths, &expected_firecracker_version)
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
        let vsock_uds = self.jail.jail_root().join(VSOCK_SOCKET);
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
        let exit_reason = bounded_stop(self.firecracker.firecracker_pid(), &vsock_uds)?;

        // Phase 4: release. Destructure to drop everything except what moves
        // into StoppedSandbox.
        let t_release = Instant::now();
        let network_helper = Arc::clone(&self.backend.network_helper);
        let RunningSandbox {
            vm_id,
            request_id,
            run_dir,
            jail,
            shared_pmem_refs,
            cgroup: _cgroup, // Drop → removes cgroup subtree.
            rootfs: _rootfs,
            scratch,
            snapshot_mount,
            client: _client,
            firecracker: _firecracker,
            permit,
            lease_guard,
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
            network_cleanup,
        } = self;
        let mut diagnostics = diagnostics;

        phase_event("stop_bounded", &vm_id, t_bounded.elapsed());
        // Join the watcher thread after destructuring (the stop flag is already
        // set above; the thread will exit on its next wake interval).
        if let Some(handle) = watcher_thread {
            let _ = handle.join();
        }
        unmount_snapshot_bind(snapshot_mount.as_deref());
        drop(jail);
        release_shared_pmem_refs(shared_pmem_refs)?;
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
            lease_guard,
            run_root,
            diagnostics,
            network_cleanup,
            network_helper,
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
                if let Err(err) = kill_and_reap_pid(self.firecracker.firecracker_pid()) {
                    self.record_forced_kill_ambiguous(&err);
                    std::mem::forget(self);
                    return Err(err);
                }
                if self.firecracker.jailer_pid() != self.firecracker.firecracker_pid() {
                    if let Err(err) = kill_and_reap_pid(self.firecracker.jailer_pid()) {
                        self.record_forced_kill_ambiguous(&err);
                        std::mem::forget(self);
                        return Err(err);
                    }
                }
            }
            StopDisposition::GuestdShutdownThenFirecrackerKill => {
                unreachable!("force kill disposition")
            }
        }

        let network_helper = Arc::clone(&self.backend.network_helper);
        let RunningSandbox {
            vm_id,
            request_id,
            run_dir,
            jail,
            shared_pmem_refs,
            cgroup: _cgroup,
            rootfs: _rootfs,
            scratch,
            snapshot_mount,
            client: _client,
            firecracker: _firecracker,
            permit,
            lease_guard,
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
            network_cleanup,
        } = self;
        let mut diagnostics = diagnostics;

        if let Some(handle) = watcher_thread {
            let _ = handle.join();
        }
        unmount_snapshot_bind(snapshot_mount.as_deref());
        drop(jail);
        release_shared_pmem_refs(shared_pmem_refs)?;
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
            lease_guard,
            run_root,
            diagnostics,
            network_cleanup,
            network_helper,
        })
    }

    fn record_forced_kill_ambiguous(&mut self, err: &FcError) {
        crate::diagnostics::record_owned(
            &mut self.diagnostics,
            Phase::Stop,
            &self.vm_id,
            self.request_id.as_deref(),
            &format!(
                "cleanup release blocked: {:?}: {err}",
                CleanupReleaseBlocker::ForcedKillAmbiguous
            ),
        );
    }
}

#[derive(Debug, Clone)]
pub(crate) struct PreparedSnapshotPaths {
    pub(crate) host_parent: PathBuf,
    pub(crate) jail_paths: SnapshotPaths,
}

impl PreparedSnapshotPaths {
    pub(crate) fn stage_paths(&self, run_dir: &Path) -> SnapshotPaths {
        let stage_parent = snapshot_stage_parent(run_dir);
        SnapshotPaths {
            vm_state: stage_parent.join(
                self.jail_paths
                    .vm_state
                    .file_name()
                    .expect("prepared jail vm_state has a file name"),
            ),
            mem: stage_parent.join(
                self.jail_paths
                    .mem
                    .file_name()
                    .expect("prepared jail mem has a file name"),
            ),
        }
    }
}

pub(crate) fn snapshot_stage_parent(run_dir: &Path) -> PathBuf {
    run_dir.join(SNAPSHOT_BIND_DEST)
}

pub(crate) fn prepare_snapshot_paths(
    paths: &SnapshotPaths,
    snapshot_root: &Path,
    create_parent: bool,
) -> Result<PreparedSnapshotPaths, FcError> {
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

    if create_parent {
        std::fs::create_dir_all(host_parent).map_err(|source| FcError::PathIo {
            path: host_parent.to_path_buf(),
            source,
        })?;
    }
    let host_parent = validate_snapshot_parent_scope(snapshot_root, host_parent)?;
    let in_jail_parent = PathBuf::from("/").join(SNAPSHOT_BIND_DEST);

    Ok(PreparedSnapshotPaths {
        host_parent,
        jail_paths: SnapshotPaths {
            vm_state: in_jail_parent.join(vm_name),
            mem: in_jail_parent.join(mem_name),
        },
    })
}

fn clean_snapshot_stage_files(paths: &SnapshotPaths) -> Result<(), FcError> {
    remove_file_if_exists(&paths.vm_state)?;
    remove_file_if_exists(&paths.mem)?;
    remove_file_if_exists(&snapshot_manifest_path(paths)?)
}

fn move_captured_snapshot_from_stage(
    stage_paths: &SnapshotPaths,
    target_paths: &SnapshotPaths,
) -> Result<(), FcError> {
    if stage_paths.vm_state != target_paths.vm_state {
        std::fs::rename(&stage_paths.vm_state, &target_paths.vm_state).map_err(|source| {
            FcError::PathIo {
                path: target_paths.vm_state.clone(),
                source,
            }
        })?;
    }
    if stage_paths.mem != target_paths.mem {
        std::fs::rename(&stage_paths.mem, &target_paths.mem).map_err(|source| FcError::PathIo {
            path: target_paths.mem.clone(),
            source,
        })?;
    }
    remove_file_if_exists(&snapshot_manifest_path(stage_paths)?)
}

fn snapshot_manifest_path(paths: &SnapshotPaths) -> Result<PathBuf, FcError> {
    let parent = paths.vm_state.parent().ok_or_else(|| {
        FcError::Config(ConfigError::InvalidValue {
            field: "snapshot.vm_state",
            reason: "path must have a parent directory".into(),
        })
    })?;
    Ok(parent.join(SNAPSHOT_MANIFEST_FILE))
}

fn remove_file_if_exists(path: &Path) -> Result<(), FcError> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(FcError::PathIo {
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn validate_snapshot_parent_scope(
    snapshot_root: &Path,
    host_parent: &Path,
) -> Result<PathBuf, FcError> {
    let canonical_root =
        std::fs::canonicalize(snapshot_root).map_err(|source| FcError::PathIo {
            path: snapshot_root.to_path_buf(),
            source,
        })?;
    let canonical_parent =
        std::fs::canonicalize(host_parent).map_err(|source| FcError::PathIo {
            path: host_parent.to_path_buf(),
            source,
        })?;

    if canonical_parent == canonical_root || !canonical_parent.starts_with(&canonical_root) {
        return Err(FcError::Config(ConfigError::InvalidValue {
            field: "snapshot",
            reason: format!(
                "snapshot directory {} must be a descendant of run_root {}",
                canonical_parent.display(),
                canonical_root.display()
            ),
        }));
    }

    Ok(canonical_parent)
}

pub(crate) fn unmount_snapshot_bind(mount_path: Option<&Path>) {
    let Some(mount_path) = mount_path else {
        return;
    };
    use nix::mount::{umount2, MntFlags};
    if let Err(e) = umount2(mount_path, MntFlags::MNT_DETACH) {
        // Skip remove_dir: if umount failed the mount is still active and
        // remove_dir will fail too, producing a confusing second warning.
        tracing::warn!(path = %mount_path.display(), err = %e, "snapshot bind unmount failed; skipping dir removal");
        return;
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
            let exit_reason = if let Err(e) = send_shutdown_request(firecracker_pid, vsock_uds) {
                tracing::warn!(
                    error = %e,
                    "vsock graceful-stop failed; SIGKILLing without ack"
                );
                ExitReason::ForceKill
            } else {
                ExitReason::NormalStop
            };
            kill_and_reap_pid(firecracker_pid)?;
            Ok(exit_reason)
        }
        StopDisposition::HostForceKill => unreachable!("normal stop disposition"),
    }
}

/// Open a fresh vsock channel, send `ShutdownRequest`, read
/// `ShutdownResponse`. Returns the guest's chosen action.
fn send_shutdown_request(
    firecracker_pid: u32,
    vsock_uds: &Path,
) -> Result<ShutdownAction, FcError> {
    let deadline = Instant::now() + SHUTDOWN_RPC_TIMEOUT;
    let mut channel = Channel::open_uds_only(vsock_uds, GUEST_PORT_DEFAULT)?;

    channel.send(&Envelope::new(ShutdownRequest { reason: None }))?;

    let resp_env: Envelope<ShutdownResponse> = channel.recv().map_err(|err| {
        protocol::recv_error_after_clean_request(err, "shutdown_response", firecracker_pid)
    })?;
    if Instant::now() > deadline {
        return Err(FcError::Protocol(WireProtocolError::ReadTimeout {
            context: "shutdown_response",
        }));
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
    u64::try_from(epoch.elapsed().as_nanos()).unwrap_or(u64::MAX)
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
    firecracker_pid: u32,
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
                firecracker_pid,
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
    pub(crate) firecracker_pid: u32,
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
        let timeout_ns = u64::try_from(timeout.as_nanos()).unwrap_or(u64::MAX);

        if idle_ns >= timeout_ns {
            tracing::info!(
                vm_id = context.vm_id,
                "idle timeout expired; issuing graceful shutdown"
            );
            context.idle_timed_out.store(true, Ordering::Relaxed);
            if let Err(e) = send_shutdown_request(context.firecracker_pid, context.vsock_uds) {
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
    fail_kill_pid_if_requested(pid)?;

    match kill(Pid::from_raw(pid as i32), Signal::SIGKILL) {
        // ESRCH = process already gone, treat as success.
        Ok(()) | Err(Errno::ESRCH) => Ok(()),
        Err(e) => Err(FcError::KillFailed {
            pid,
            source: std::io::Error::from_raw_os_error(e as i32),
        }),
    }
}

#[cfg(debug_assertions)]
fn fail_kill_pid_if_requested(pid: u32) -> Result<(), FcError> {
    if std::env::var(FORCE_KILL_EPERM_FOR_PID_ENV).ok().as_deref() == Some(&pid.to_string()) {
        return Err(FcError::KillFailed {
            pid,
            source: std::io::Error::from(std::io::ErrorKind::PermissionDenied),
        });
    }
    Ok(())
}

#[cfg(not(debug_assertions))]
fn fail_kill_pid_if_requested(_pid: u32) -> Result<(), FcError> {
    Ok(())
}

/// Send `SIGKILL` to `pid` and reap it when it is one of this process's
/// children. Treat `ECHILD` as success because daemonized/new-pid-ns paths are
/// reaped by their real parent.
pub(crate) fn kill_and_reap_pid(pid: u32) -> Result<(), FcError> {
    use nix::errno::Errno;
    use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};
    use nix::unistd::Pid;

    kill_pid(pid)?;
    if pid == 0 {
        return Ok(());
    }

    let reap_pid = Pid::from_raw(pid as i32);
    let timeout = REAP_TIMEOUT;
    let deadline = Instant::now() + timeout;
    let mut backoff = ReapPollBackoff::new();
    loop {
        match waitpid(reap_pid, Some(WaitPidFlag::WNOHANG)) {
            Ok(WaitStatus::Exited(_, _))
            | Ok(WaitStatus::Signaled(_, _, _))
            | Err(Errno::ECHILD)
            | Err(Errno::ESRCH) => return Ok(()),
            Ok(WaitStatus::StillAlive) => {
                if Instant::now() >= deadline {
                    return Err(FcError::ReapTimeout { pid, timeout });
                }
                std::thread::sleep(reap_sleep_duration(&mut backoff, deadline));
            }
            Ok(_) => return Ok(()),
            Err(e) => {
                return Err(FcError::ReapFailed {
                    pid,
                    source: std::io::Error::from_raw_os_error(e as i32),
                });
            }
        }
    }
}

struct ReapPollBackoff {
    next: Duration,
}

impl ReapPollBackoff {
    fn new() -> Self {
        Self {
            next: REAP_INITIAL_POLL_DELAY,
        }
    }

    fn next_delay(&mut self) -> Duration {
        let delay = self.next;
        self.next = self.next.saturating_mul(2).min(REAP_MAX_POLL_DELAY);
        delay
    }
}

fn reap_sleep_duration(backoff: &mut ReapPollBackoff, deadline: Instant) -> Duration {
    let remaining = deadline.saturating_duration_since(Instant::now());
    backoff.next_delay().min(remaining)
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
    fn snapshot_parent_scope_accepts_directory_under_run_root() {
        let run_root = tempfile::tempdir().expect("run root");
        let snapshot_parent = run_root.path().join("warm/snapshot");
        std::fs::create_dir_all(&snapshot_parent).expect("snapshot parent");

        let scoped =
            validate_snapshot_parent_scope(run_root.path(), &snapshot_parent).expect("scope");

        assert_eq!(
            scoped,
            snapshot_parent
                .canonicalize()
                .expect("canonical snapshot parent")
        );
    }

    #[test]
    fn snapshot_parent_scope_rejects_directory_outside_run_root() {
        let run_root = tempfile::tempdir().expect("run root");
        let outside = tempfile::tempdir().expect("outside snapshot parent");

        let err = validate_snapshot_parent_scope(run_root.path(), outside.path())
            .expect_err("outside snapshot parent must be rejected");

        assert_invalid_snapshot_scope(err);
    }

    #[test]
    fn snapshot_parent_scope_rejects_run_root_itself() {
        let run_root = tempfile::tempdir().expect("run root");

        let err = validate_snapshot_parent_scope(run_root.path(), run_root.path())
            .expect_err("run root itself must not be a snapshot parent");

        assert_invalid_snapshot_scope(err);
    }

    #[test]
    fn snapshot_parent_scope_rejects_symlink_escape_from_run_root() {
        let run_root = tempfile::tempdir().expect("run root");
        let outside = tempfile::tempdir().expect("outside snapshot parent");
        let link = run_root.path().join("snapshot-link");
        std::os::unix::fs::symlink(outside.path(), &link).expect("symlink");

        let err = validate_snapshot_parent_scope(run_root.path(), &link)
            .expect_err("symlink escape must be rejected");

        assert_invalid_snapshot_scope(err);
    }

    fn assert_invalid_snapshot_scope(err: FcError) {
        assert!(
            matches!(
                err,
                FcError::Config(ConfigError::InvalidValue {
                    field: "snapshot",
                    ..
                })
            ),
            "expected invalid snapshot scope error, got {err:?}"
        );
    }

    #[test]
    fn reap_poll_backoff_starts_fast_and_caps_at_twenty_ms() {
        let mut backoff = ReapPollBackoff::new();
        let delays = (0..8).map(|_| backoff.next_delay()).collect::<Vec<_>>();

        assert_eq!(
            delays,
            vec![
                Duration::from_millis(1),
                Duration::from_millis(2),
                Duration::from_millis(4),
                Duration::from_millis(8),
                Duration::from_millis(16),
                Duration::from_millis(20),
                Duration::from_millis(20),
                Duration::from_millis(20),
            ]
        );
    }

    #[test]
    fn reap_sleep_never_exceeds_remaining_deadline() {
        let mut backoff = ReapPollBackoff {
            next: Duration::from_millis(20),
        };
        let deadline = Instant::now() + Duration::from_millis(3);

        assert!(reap_sleep_duration(&mut backoff, deadline) <= Duration::from_millis(3));
    }

    #[test]
    fn reap_poll_schedule_observes_exit_within_next_backoff_slot() {
        for exit_after in [
            Duration::ZERO,
            Duration::from_millis(1),
            Duration::from_millis(3),
            Duration::from_millis(5),
            Duration::from_millis(9),
            Duration::from_millis(17),
            Duration::from_millis(37),
        ] {
            let observed_at = simulated_reap_observation_delay(exit_after);
            let bound = exit_after + first_backoff_delay_at_least(exit_after);

            assert!(
                observed_at <= bound,
                "exit_after={exit_after:?} observed_at={observed_at:?} bound={bound:?}"
            );
        }
    }

    fn simulated_reap_observation_delay(exit_after: Duration) -> Duration {
        let mut elapsed = Duration::ZERO;
        let mut backoff = ReapPollBackoff::new();
        loop {
            if elapsed >= exit_after {
                return elapsed;
            }
            elapsed += backoff.next_delay();
        }
    }

    fn first_backoff_delay_at_least(delay: Duration) -> Duration {
        let mut backoff = ReapPollBackoff::new();
        loop {
            let next = backoff.next_delay();
            if next >= delay || next == REAP_MAX_POLL_DELAY {
                return next;
            }
        }
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
                        firecracker_pid: std::process::id(),
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
