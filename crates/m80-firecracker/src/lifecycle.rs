//! [`RunningSandbox`] and [`StoppedSandbox`] method implementations.
//! Transitions consume the prior handle (move semantics).

use std::path::{Path, PathBuf};
use std::time::Duration;

use m80_firecracker_client::InstanceAction;
use m80_proto::{Envelope, ExecRequest, ExecResponse};
use m80_storage::ChangeSet;

use crate::error::FcError;
use crate::runroot::unix_ms_now;
use crate::types::{RunningSandbox, StoppedSandbox};

/// How long to wait for Firecracker to exit after `SendCtrlAltDel`.
const GRACEFUL_STOP_TIMEOUT: Duration = Duration::from_secs(30);

/// Polling interval when waiting for a pid to exit.
const POLL_INTERVAL: Duration = Duration::from_millis(100);

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
        self.channel.send(&envelope)?;
        let resp_env: Envelope<ExecResponse> = self.channel.recv()?;
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

        // Phase 2: bounded_stop.
        bounded_stop(&self.client, self.firecracker.firecracker_pid)?;

        // Phase 4: release. Destructure to drop everything except what moves
        // into StoppedSandbox.
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

/// Graceful stop on x86_64 (SendCtrlAltDel + wait), SIGKILL fallback.
fn bounded_stop(
    client: &m80_firecracker_client::Client,
    firecracker_pid: u32,
) -> Result<(), FcError> {
    #[cfg(target_arch = "x86_64")]
    {
        // Best-effort: ignore the error (Firecracker may already be exiting).
        let _ = client.instance_action(InstanceAction::SendCtrlAltDel);
        if wait_for_pid_exit(firecracker_pid, GRACEFUL_STOP_TIMEOUT) {
            return Ok(());
        }
        // Graceful stop timed out; fall through to SIGKILL.
        tracing::warn!(firecracker_pid, "graceful stop timed out; sending SIGKILL");
    }

    #[cfg(not(target_arch = "x86_64"))]
    let _ = client;

    kill_pid(firecracker_pid)
}

/// Poll `/proc/<pid>` until it disappears or `timeout` elapses.
/// Returns `true` if the pid exited before the deadline.
fn wait_for_pid_exit(pid: u32, timeout: Duration) -> bool {
    use std::time::Instant;
    let deadline = Instant::now() + timeout;
    let proc = PathBuf::from(format!("/proc/{pid}"));
    while proc.exists() {
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(POLL_INTERVAL);
    }
    true
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
