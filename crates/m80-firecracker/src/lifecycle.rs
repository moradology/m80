//! [`RunningSandbox`] and [`StoppedSandbox`] method implementations.
//! Transitions consume the prior handle (move semantics).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use m80_proto::{Envelope, ExecRequest, ExecResponse, ShutdownAction, ShutdownRequest, ShutdownResponse};
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
    /// Multiple sequential execs over the same channel are supported in v0.1;
    /// the channel stays open between calls. Pipelining (concurrent exec) is
    /// not supported.
    pub fn exec(&mut self, req: ExecRequest) -> Result<ExecResponse, FcError> {
        let envelope = Envelope::new(req);
        let t = Instant::now();
        self.channel.send(&envelope)?;
        phase_event("exec_send", &self.vm_id, t.elapsed());
        let t = Instant::now();
        let resp_env: Envelope<ExecResponse> = self.channel.recv()?;
        phase_event("exec_recv", &self.vm_id, t.elapsed());
        Ok(resp_env.payload)
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
        } = self;
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
        } = self;

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
