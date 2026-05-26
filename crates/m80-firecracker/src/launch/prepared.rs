use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize};
use std::sync::Arc;
use std::time::Instant;

use m80_cgroup::Subtree;
use m80_firecracker_client::{Client, InstanceAction};
use m80_jailer::{JailedFirecracker, MaterializedJail};
use m80_observability::Phase;

use crate::error::FcError;
use crate::layout::console_log_path;
use crate::lifecycle::{monotonic_ns, phase_13_pmem_guest_mount, spawn_idle_watcher};
use crate::types::{RunningSandbox, SandboxConfig, StoragePrep};

use super::failure_cleanup::{
    LaunchNetworkCleanupGuard, LaunchProcessCleanupGuard, LaunchRunDirCleanupGuard,
};
use super::{phase_12b_ready_accept, record_post_launch_resource_snapshot};

macro_rules! diag_phase {
    ($current_phase:ident, $diag:expr, $vid:expr, $rid:expr, $phase:expr, $name:literal, $body:expr) => {{
        $current_phase = $name;
        crate::diagnostics::phase_result($diag, $phase, $name, $vid, $rid, || $body)
    }};
}

/// A cold-boot sandbox prepared through Firecracker REST configuration but not
/// yet started.
///
/// `PreparedSandbox` owns the admission permit and every host-side resource
/// created by [`crate::Sandbox::prepare`]. Dropping it runs the same pre-running
/// cleanup guards used by failed launch; prefer [`PreparedSandbox::abort`] when
/// the caller intentionally decides not to start the VM.
pub struct PreparedSandbox {
    pub(super) vm_id: String,
    pub(super) request_id: Option<String>,
    pub(super) run_dir: PathBuf,
    pub(super) jail: MaterializedJail,
    pub(super) storage: StoragePrep,
    pub(super) cgroup: Option<Subtree>,
    pub(super) client: Client,
    pub(super) firecracker: JailedFirecracker,
    pub(super) permit: crate::types::AdmissionPermit,
    pub(super) lease_guard: crate::runroot::LeaseGuard,
    pub(super) backend: Arc<crate::Backend>,
    pub(super) diagnostics: Option<m80_observability::Diagnostics>,
    pub(super) ready_listener: UnixListener,
    pub(super) ready_uds: PathBuf,
    pub(super) vsock_uds: PathBuf,
    pub(super) config: SandboxConfig,
    pub(super) process_cleanup: LaunchProcessCleanupGuard,
    pub(super) run_dir_cleanup: LaunchRunDirCleanupGuard,
    pub(super) network_cleanup: Option<LaunchNetworkCleanupGuard>,
}

impl PreparedSandbox {
    /// Return the VM id selected during admission.
    #[must_use]
    pub fn vm_id(&self) -> &str {
        &self.vm_id
    }

    /// Return the per-VM run directory.
    #[must_use]
    pub fn run_dir(&self) -> &Path {
        &self.run_dir
    }

    /// Start the prepared VM and wait for guest readiness.
    pub fn start(self) -> Result<RunningSandbox, FcError> {
        let PreparedSandbox {
            vm_id,
            request_id,
            run_dir,
            jail,
            storage,
            cgroup,
            client,
            firecracker,
            permit,
            lease_guard,
            backend,
            mut diagnostics,
            ready_listener,
            ready_uds,
            vsock_uds,
            config,
            mut process_cleanup,
            mut run_dir_cleanup,
            mut network_cleanup,
        } = self;
        let summary_run_dir = run_dir.clone();
        let summary_vm_id = vm_id.clone();
        let summary_request_id = request_id.clone();
        let mut current_phase: &'static str = "phase_12a_instance_start";
        let result = (|| -> Result<RunningSandbox, FcError> {
            // Phase 12a: InstanceStart.
            diag_phase!(
                current_phase,
                &mut diagnostics,
                &vm_id,
                request_id.as_deref(),
                Phase::Boot,
                "phase_12a_instance_start",
                {
                    client
                        .instance_action(InstanceAction::InstanceStart)
                        .map_err(FcError::Client)
                }
            )?;
            crate::diagnostics::record_owned(
                &mut diagnostics,
                Phase::Boot,
                &vm_id,
                request_id.as_deref(),
                "instance started",
            );

            // Phase 12b: accept the inverted-readiness signal from m80-guestd.
            // The signal is emitted after guestd has bound the exec listener, so
            // launch does not consume a dummy exec-channel connection before the
            // caller's first real request.
            diag_phase!(
                current_phase,
                &mut diagnostics,
                &vm_id,
                request_id.as_deref(),
                Phase::Ready,
                "phase_12b_ready_accept",
                {
                    phase_12b_ready_accept(
                        &ready_listener,
                        &ready_uds,
                        &vsock_uds,
                        &console_log_path(&run_dir),
                        &vm_id,
                    )
                }
            )?;
            diag_phase!(
                current_phase,
                &mut diagnostics,
                &vm_id,
                request_id.as_deref(),
                Phase::Ready,
                "phase_13_pmem_guest_mount",
                {
                    phase_13_pmem_guest_mount(
                        &vsock_uds,
                        &vm_id,
                        request_id.as_deref(),
                        firecracker.firecracker_pid(),
                        &config.pmem_layers,
                    )
                }
            )?;
            record_post_launch_resource_snapshot(
                &mut diagnostics,
                &vm_id,
                request_id.as_deref(),
                firecracker.firecracker_pid(),
                cgroup.is_some(),
            );
            crate::diagnostics::record_owned(
                &mut diagnostics,
                Phase::Ready,
                &vm_id,
                request_id.as_deref(),
                "guestd ready",
            );

            let last_activity_ns = Arc::new(AtomicU64::new(monotonic_ns()));
            let active_execs = Arc::new(AtomicUsize::new(0));
            let idle_timed_out = Arc::new(AtomicBool::new(false));
            let lifetime_expired = Arc::new(AtomicBool::new(false));
            let watcher_stop = Arc::new(AtomicBool::new(false));
            let born_at = Instant::now();
            let born_at_ns = monotonic_ns();
            let max_lifetime = config.max_lifetime;
            let watcher_thread =
                (config.idle_timeout.is_some() || max_lifetime.is_some()).then(|| {
                    spawn_idle_watcher(
                        config.idle_timeout,
                        max_lifetime,
                        born_at_ns,
                        vsock_uds.clone(),
                        firecracker.firecracker_pid(),
                        Arc::clone(&last_activity_ns),
                        Arc::clone(&active_execs),
                        Arc::clone(&idle_timed_out),
                        Arc::clone(&lifetime_expired),
                        Arc::clone(&watcher_stop),
                        vm_id.clone(),
                    )
                });

            let kill_guard = crate::types::ForceKillGuard::new(
                vm_id.clone(),
                firecracker.firecracker_pid(),
                firecracker.jailer_pid(),
                Arc::clone(&watcher_stop),
                None,
            );
            let preallocated_drive_slots = storage.preallocated_drive_slots.len() as u8;
            process_cleanup.disarm();
            let network_cleanup_enabled = network_cleanup.is_some();
            if let Some(guard) = &mut network_cleanup {
                guard.disarm();
            }
            run_dir_cleanup.disarm();
            crate::ops_metrics::record_launch();
            Ok(RunningSandbox {
                vm_id,
                request_id,
                run_dir,
                jail,
                shared_pmem_refs: storage.shared_pmem_refs,
                cgroup,
                rootfs: storage.rootfs,
                scratch: storage.scratch,
                snapshot_mount: None,
                client,
                firecracker,
                permit,
                lease_guard,
                backend,
                born_at,
                max_lifetime,
                last_activity_ns,
                active_execs,
                idle_timed_out,
                lifetime_expired,
                watcher_stop,
                watcher_thread,
                diagnostics,
                preallocated_drive_slots,
                one_shot: config.one_shot,
                one_shot_consumed: false,
                kill_guard,
                network_cleanup: network_cleanup_enabled,
            })
        })();
        if let Err(err) = &result {
            crate::diagnostics::record_failure_summary_best_effort(
                &summary_run_dir,
                &summary_vm_id,
                current_phase,
                summary_request_id.as_deref(),
                err,
            );
        }
        result
    }

    /// Tear down the prepared VM before `InstanceStart` and delete its run
    /// directory.
    pub fn abort(mut self) -> Result<(), FcError> {
        crate::diagnostics::record_owned(
            &mut self.diagnostics,
            Phase::Stop,
            &self.vm_id,
            self.request_id.as_deref(),
            "prepared sandbox abort started",
        );
        let run_dir = self.run_dir.clone();
        self.run_dir_cleanup.disarm();
        drop(self);
        match std::fs::remove_dir_all(&run_dir) {
            Ok(()) => Ok(()),
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(source) => Err(FcError::PathIo {
                path: run_dir,
                source,
            }),
        }
    }
}

impl std::fmt::Debug for PreparedSandbox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreparedSandbox")
            .field("vm_id", &self.vm_id)
            .field("run_dir", &self.run_dir)
            .finish_non_exhaustive()
    }
}
