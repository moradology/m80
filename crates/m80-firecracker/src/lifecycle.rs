//! [`RunningSandbox`] and [`StoppedSandbox`] method implementations.
//! Transitions consume the prior handle (move semantics).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use m80_proto::{Envelope, ExecRequest, ExecResponse, ShutdownAction, ShutdownRequest, ShutdownResponse};
use m80_snapshot::{capture as snapshot_capture, CaptureRequest, SnapshotKind, SnapshotPaths};
use m80_storage::ChangeSet;
use m80_vsock::{Channel, GUEST_PORT_DEFAULT};

use crate::error::FcError;
use crate::runroot::unix_ms_now;
use crate::timing::phase_event;
use crate::types::{RunningSandbox, StoppedSandbox};

/// Per-attempt deadline for the shutdown vsock round-trip (open UDS,
/// send request, read response).
const SHUTDOWN_RPC_TIMEOUT: Duration = Duration::from_secs(5);

impl RunningSandbox {
    /// Return the VM id for this sandbox.
    pub fn vm_id(&self) -> &str {
        &self.vm_id
    }

    /// Send one exec request to the in-VM daemon and return the response.
    ///
    /// The VM stays alive after the call returns; sequential execs on the same
    /// `RunningSandbox` share the same filesystem state. Pipelining (concurrent
    /// exec) is not supported — the borrow checker enforces one in-flight exec
    /// at a time via `&mut self`. Call `stop()` (or drop the sandbox) to tear
    /// down the VM.
    ///
    /// Returns `FcError::IdleTimedOut` if the idle-timeout watcher has already
    /// fired. The VM has been gracefully shut down; the caller must drop or
    /// `stop()` the sandbox.
    pub fn exec(&mut self, req: ExecRequest) -> Result<ExecResponse, FcError> {
        if self.idle_timed_out.load(Ordering::Relaxed) {
            return Err(FcError::IdleTimedOut);
        }
        // Touch last-activity at the start of every exec so a long-running
        // exec doesn't trip the idle watcher mid-flight.
        self.last_activity_ns
            .store(monotonic_ns(), Ordering::Relaxed);
        let envelope = Envelope::new(req);
        let t = Instant::now();
        self.channel.send(&envelope)?;
        phase_event("exec_send", &self.vm_id, t.elapsed());
        let t = Instant::now();
        let resp_env: Envelope<ExecResponse> = self.channel.recv()?;
        phase_event("exec_recv", &self.vm_id, t.elapsed());
        // Touch last-activity at completion too, so the watcher deadline is
        // reset from the end of the call (not its start).
        self.last_activity_ns
            .store(monotonic_ns(), Ordering::Relaxed);
        Ok(resp_env.payload)
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
    /// # Errors
    ///
    /// Returns `FcError::Snapshot` if either REST call fails. The caller
    /// should treat any error as the VM being in an unknown state and call
    /// `force_kill()`.
    pub fn capture(&self, paths: SnapshotPaths) -> Result<(), FcError> {
        let fc_socket = self.jail.jail_path.join("firecracker.sock");
        snapshot_capture(CaptureRequest {
            fc_socket: &fc_socket,
            paths,
            kind: SnapshotKind::Full,
        })
        .map_err(FcError::Snapshot)
    }

    /// Four-phase teardown:
    /// 1. `admission_fence` — no new exec accepted (no-op in v0.1).
    /// 2. `bounded_stop` — graceful on x86_64 (`SendCtrlAltDel` + wait 30 s),
    ///    forced (`SIGKILL`) on aarch64 or after timeout.
    /// 3. Optional `extract_changes` — NOT done here; caller calls
    ///    [`StoppedSandbox::extract_changes`] after receiving the `StoppedSandbox`.
    /// 4. `release` — foundation resources dropped, permit and scratch moved
    ///    into the returned `StoppedSandbox`.
    pub fn stop(self) -> Result<StoppedSandbox, FcError> {
        let run_root = self.backend.config.run_root.clone();
        let vm_id_for_event = self.vm_id.clone();
        let vsock_uds = self.jail.jail_path.join("vsock.sock");

        // Signal the idle-watcher thread to exit before teardown so it does
        // not race with the shutdown we are about to send.
        self.watcher_stop.store(true, Ordering::Relaxed);

        // Phase 2: bounded_stop.
        let t = Instant::now();
        bounded_stop(self.firecracker.firecracker_pid, &vsock_uds)?;
        phase_event("stop_bounded", &vm_id_for_event, t.elapsed());

        // Phase 4: release. Destructure to drop everything except what moves
        // into StoppedSandbox.
        let t = Instant::now();
        let RunningSandbox {
            vm_id,
            run_dir,
            jail: _jail,       // Drop → unmounts bind mounts + removes jail dir.
            cgroup: _cgroup,   // Drop → removes cgroup subtree.
            channel: _channel, // Drop → removes host vsock UDS.
            rootfs: _rootfs,
            scratch,
            client: _client,
            firecracker: _firecracker,
            permit,
            backend: _backend,
            last_activity_ns: _last_activity_ns,
            idle_timed_out: _idle_timed_out,
            watcher_stop: _watcher_stop,
            watcher_thread,
        } = self;

        // Join the watcher thread after destructuring (the stop flag is already
        // set above; the thread will exit on its next wake interval).
        if let Some(handle) = watcher_thread {
            let _ = handle.join();
        }
        phase_event("stop_release", &vm_id_for_event, t.elapsed());

        Ok(StoppedSandbox {
            vm_id,
            run_dir,
            scratch,
            permit,
            run_root,
        })
    }

    /// Last-resort: SIGKILL the firecracker and jailer pids immediately.
    ///
    /// The run-dir is preserved for offline inspection. The caller decides
    /// whether to delete it via [`StoppedSandbox::delete`] or preserve it
    /// further via [`StoppedSandbox::preserve_for_triage`].
    pub fn force_kill(self) -> Result<StoppedSandbox, FcError> {
        let run_root = self.backend.config.run_root.clone();

        // Signal the watcher to exit before killing the process.
        self.watcher_stop.store(true, Ordering::Relaxed);

        kill_pid(self.firecracker.firecracker_pid)?;
        kill_pid(self.firecracker.jailer_pid)?;

        let RunningSandbox {
            vm_id,
            run_dir,
            jail: _jail,
            cgroup: _cgroup,
            channel: _channel,
            rootfs: _rootfs,
            scratch,
            client: _client,
            firecracker: _firecracker,
            permit,
            backend: _backend,
            last_activity_ns: _last_activity_ns,
            idle_timed_out: _idle_timed_out,
            watcher_stop: _watcher_stop,
            watcher_thread,
        } = self;

        if let Some(handle) = watcher_thread {
            let _ = handle.join();
        }

        Ok(StoppedSandbox {
            vm_id,
            run_dir,
            scratch,
            permit,
            run_root,
        })
    }
}

impl StoppedSandbox {
    /// Return the per-VM run directory.
    pub fn run_dir(&self) -> &Path {
        &self.run_dir
    }

    /// Opt-in change extraction from the workspace scratch image.
    ///
    /// Returns `FcError::Config` if no workspace (scratch image) was
    /// configured for this sandbox.
    pub fn extract_changes(&self, into: &Path) -> Result<ChangeSet, FcError> {
        let scratch = self.scratch.as_ref().ok_or_else(|| {
            FcError::Config("no scratch image: workspace was not configured".into())
        })?;
        let cs = m80_storage::Scratch::extract(scratch.path(), into)?;
        Ok(cs)
    }

    /// Remove the per-VM run-dir and release the admission permit.
    pub fn delete(self) -> Result<(), FcError> {
        std::fs::remove_dir_all(&self.run_dir)?;
        // `self` drops here; AdmissionPermit::drop returns the slot.
        Ok(())
    }

    /// Move the per-VM run-dir to `.preserved/<unix_ms>-<vm_id>/` for
    /// offline triage. The admission permit is released. Returns the new path.
    pub fn preserve_for_triage(self) -> Result<PathBuf, FcError> {
        let preserved_parent = self.run_root.join(".preserved");
        std::fs::create_dir_all(&preserved_parent)?;

        let ts = unix_ms_now();
        let dest = preserved_parent.join(format!("{ts}-{}", self.vm_id));
        std::fs::rename(&self.run_dir, &dest)?;

        // `self` drops here; permit returned.
        Ok(dest)
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
fn bounded_stop(firecracker_pid: u32, vsock_uds: &Path) -> Result<(), FcError> {
    if let Err(e) = send_shutdown_request(vsock_uds) {
        tracing::warn!(
            error = %e,
            "vsock graceful-stop failed; SIGKILLing without ack"
        );
    }
    kill_pid(firecracker_pid)
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
/// the elapsed time since `last_activity_ns` against `timeout`. When the
/// deadline expires, it sends a graceful shutdown request over `vsock_uds`
/// (best-effort; logs on failure) and sets `idle_timed_out` so the next
/// `exec` returns `FcError::IdleTimedOut`.
///
/// The thread exits when `stop_flag` is set (by `stop()` or `force_kill()`).
pub(crate) fn spawn_idle_watcher(
    timeout: Duration,
    vsock_uds: std::path::PathBuf,
    last_activity_ns: Arc<AtomicU64>,
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
            &vsock_uds,
            &last_activity_ns,
            &idle_timed_out,
            &stop_flag,
            &vm_id,
        );
    })
}

/// Inner loop of the idle-timeout watcher. Extracted so it's testable
/// without spawning a real thread.
pub(crate) fn idle_watcher_loop(
    timeout: Duration,
    poll_interval: Duration,
    vsock_uds: &std::path::Path,
    last_activity_ns: &AtomicU64,
    idle_timed_out: &AtomicBool,
    stop_flag: &AtomicBool,
    vm_id: &str,
) {
    loop {
        std::thread::sleep(poll_interval);

        if stop_flag.load(Ordering::Relaxed) {
            return;
        }

        let last_ns = last_activity_ns.load(Ordering::Relaxed);
        let now_ns = monotonic_ns();
        let idle_ns = now_ns.saturating_sub(last_ns);
        let timeout_ns = timeout.as_nanos() as u64;

        if idle_ns >= timeout_ns {
            tracing::info!(vm_id, "idle timeout expired; issuing graceful shutdown");
            idle_timed_out.store(true, Ordering::Relaxed);
            if let Err(e) = send_shutdown_request(vsock_uds) {
                tracing::warn!(
                    vm_id,
                    error = %e,
                    "idle watcher: graceful shutdown failed (VM may already be stopped)"
                );
            }
            return;
        }
    }
}

/// Send `SIGKILL` to `pid`. Treats `ESRCH` (no such process) as success.
fn kill_pid(pid: u32) -> Result<(), FcError> {
    use nix::errno::Errno;
    use nix::sys::signal::{kill, Signal};
    use nix::unistd::Pid;

    match kill(Pid::from_raw(pid as i32), Signal::SIGKILL) {
        Ok(()) => Ok(()),
        Err(Errno::ESRCH) => Ok(()), // Process already gone.
        Err(e) => Err(FcError::Io(std::io::Error::from_raw_os_error(e as i32))),
    }
}
