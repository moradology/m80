//! [`Sandbox::launch`] and the 12-phase preboot pipeline.
//!
//! # v0.1 simplifications
//!
//! - **Phase 6**: `OutboundNat` is rejected with a clear error; `m80-net-outbound`
//!   is deferred to v0.2 and would panic if called.
//!
//! - **Phase 7 (guest config injection)**: no-op in v0.1 because `OutboundNat`
//!   is rejected in phase 6 before we reach this step.
//!
//! - **Phase 12b (ready accept)**: m80-guestd connects out to the host on
//!   `m80_proto::READY_PORT_DEFAULT` immediately after binding its
//!   listener; the host pre-creates a `UnixListener` at
//!   `<vsock_uds>_<READY_PORT>` (Phase 11b) and `accept()`s. Event-driven,
//!   no polling, no muxer EAGAIN race.

use std::io::Read;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize};
use std::sync::Arc;
use std::time::{Duration, Instant};

use m80_cgroup::{Limits, Subtree};
use m80_firecracker_client::{Client, InstanceAction};
use m80_jailer::{BindMode, Binding, JailerConfig, JailerSocket, Plan};
use m80_net_mode::VmNetworkMode;
use m80_observability::Phase;
use m80_preflight::Discovery;
use m80_proto::READY_PORT_DEFAULT;
use m80_snapshot::{restore as snapshot_restore, RestoreRequest, SnapshotPaths};
use m80_vsock::{Channel, GUEST_PORT_DEFAULT};

use crate::diagnostics::phase;
use crate::error::{ConfigError, FcError, WireProtocolError};
use crate::layout::{
    console_log_path, firecracker_api_socket_path, preallocated_drive_slot_filename, run_dir_path,
    vsock_socket_path,
};
use crate::lifecycle::{bind_snapshot_parent_into_jail, monotonic_ns, spawn_idle_watcher};
use crate::preboot::{apply_preboot_puts, plan_preboot_puts};
use crate::runroot::write_ownership_lock;
use crate::storage_prep::phase_3_storage_prep;
use crate::types::{
    CgroupMode, RealizedNetwork, RunningSandbox, Sandbox, SandboxConfig, StoragePrep,
};

mod failure_cleanup;

use failure_cleanup::{LaunchProcessCleanupGuard, LaunchRunDirCleanupGuard};

/// Record a diagnostics-annotated phase result.
///
/// Takes `diagnostics` (`&mut Option<Diagnostics>`), `vm_id` (`&str`), and
/// `request_id` (`Option<&str>`) as explicit arguments so the macro can be
/// defined at module scope — Rust `macro_rules!` cannot capture local
/// variables from the caller's scope.
macro_rules! diag_phase {
    ($diag:expr, $vid:expr, $rid:expr, $phase:expr, $name:literal, $body:expr) => {
        crate::diagnostics::phase_result($diag, $phase, $name, $vid, $rid, || $body)
    };
}

/// Ready probe: total timeout.
///
/// Bounds how long phase_12b_ready_accept will wait for m80-guestd's
/// outbound connect to land. Has to cover guest kernel boot + (for
/// ubuntu) systemd reaching multi-user.target, with comfortable margin
/// for stress-loaded hosts.
const READY_TIMEOUT: Duration = Duration::from_secs(60);

/// Accept-loop sleep when the listener is non-blocking and no connection
/// has arrived yet. This is host-local (m80 polling its own UnixListener),
/// not interaction with Firecracker's muxer — no EAGAIN race possible.
const READY_ACCEPT_POLL: Duration = Duration::from_millis(10);

/// Read-deadline for the proto-version byte after `accept()`.
const READY_VERSION_READ_TIMEOUT: Duration = Duration::from_secs(2);
const API_SOCKET_TIMEOUT: Duration = Duration::from_secs(5);

impl Sandbox {
    /// Standalone constructor for callers without a `Backend`.
    ///
    /// Deferred to v0.2. The admission semaphore is bypassed in this path,
    /// but constructing a minimal Backend without a Discovery from preflight
    /// is not supported in v0.1. Use `Backend::admit` instead.
    pub fn new(_config: SandboxConfig) -> Result<Sandbox, FcError> {
        Err(FcError::Config(ConfigError::Other(
            "Sandbox::new is not supported in v0.1; use Backend::admit(config).launch()".into(),
        )))
    }

    /// Resolve or auto-generate the VM identifier.
    fn resolve_vm_id(&self) -> String {
        self.config.vm_id.clone().unwrap_or_else(|| {
            let pid = std::process::id();
            let ts = crate::runroot::unix_ms_now();
            format!("vm-{pid}-{ts}")
        })
    }

    /// Run the strict 12-phase preboot pipeline (Created → Running).
    ///
    /// Consumes `self` so a failed launch cannot be retried — the admission
    /// permit is dropped on any error path.
    pub fn launch(self) -> Result<RunningSandbox, FcError> {
        let vm_id = self.resolve_vm_id();
        let backend_config = &self.backend.config;
        let run_root = &backend_config.run_root;

        // Phase 1: run-root prep.
        let run_dir = phase("phase_1_run_root_prep", &vm_id, || {
            phase_1_run_root_prep(run_root, &vm_id)
        })?;
        let mut run_dir_cleanup = LaunchRunDirCleanupGuard::new(&vm_id, run_dir.clone());
        let request_id = self.config.request_id.clone();
        let mut diagnostics = crate::diagnostics::open(&run_dir, &vm_id, request_id.as_deref());

        // Phase 2: lease acquisition. Keep the guard inside RunningSandbox so
        // ownership.lock covers the whole VM lifetime, not just launch.
        let lease_guard = phase("phase_2_lease", &vm_id, || write_ownership_lock(&run_dir))?;

        // Phase 3: storage prep. Artifact sha256 verification is owned by
        // m80-preflight before Backend construction, not by each launch.
        let storage = diag_phase!(
            &mut diagnostics,
            &vm_id,
            request_id.as_deref(),
            Phase::StoragePrepare,
            "phase_3_storage_prep",
            {
                phase_3_storage_prep(
                    &vm_id,
                    &backend_config.discovery.rootfs,
                    &self.config,
                    &run_dir,
                )
            }
        )?;
        crate::diagnostics::record_owned(
            &mut diagnostics,
            Phase::StoragePrepare,
            &vm_id,
            request_id.as_deref(),
            "storage prepared",
        );

        // Phase 4: jailer materialize.
        let jail = diag_phase!(
            &mut diagnostics,
            &vm_id,
            request_id.as_deref(),
            Phase::Boot,
            "phase_4_jailer_materialize",
            {
                phase_4_jailer_materialize(JailerMaterializeInput {
                    jailer_bin: &backend_config.discovery.jailer_bin,
                    jailer_harden_bin: &backend_config.discovery.jailer_harden_bin,
                    firecracker_bin: &backend_config.discovery.firecracker_bin,
                    uid: backend_config.jail_uid,
                    gid: backend_config.jail_gid,
                    run_dir: &run_dir,
                    kernel: &backend_config.discovery.kernel,
                    storage: &storage,
                    daemonize: self.config.daemonize,
                    netns_path: join_netns_path(&self.config.network),
                })
            }
        )?;

        // Phase 5: probe cgroup availability (creation happens after phase 9).
        diag_phase!(
            &mut diagnostics,
            &vm_id,
            request_id.as_deref(),
            Phase::HostPreflight,
            "phase_5_cgroup_probe",
            { phase_5_cgroup_probe(backend_config.cgroup_mode) }
        )?;

        // Phase 6: resolve network mode.
        let net = diag_phase!(
            &mut diagnostics,
            &vm_id,
            request_id.as_deref(),
            Phase::NetworkPrepare,
            "phase_6_network_realize",
            { phase_6_network_realize(&self.config) }
        )?;
        let network_message = match &net {
            RealizedNetwork::NoEgress => "network prepared".to_owned(),
            RealizedNetwork::JoinNetns { netns_path } => {
                format!("network prepared: join_netns {}", netns_path.display())
            }
        };
        crate::diagnostics::record_owned(
            &mut diagnostics,
            Phase::NetworkPrepare,
            &vm_id,
            request_id.as_deref(),
            &network_message,
        );

        // Phase 7: guest config injection — no-op in v0.1. OutboundNat is
        // rejected in phase 6 before preboot REST PUTs are built.

        // Phase 8: compute the API socket path (inside the jail root).
        let api_socket =
            firecracker_api_socket_path(&run_dir, &backend_config.discovery.firecracker_bin);

        // Phase 9: jailer exec's firecracker. Returns live pids.
        let firecracker = diag_phase!(
            &mut diagnostics,
            &vm_id,
            request_id.as_deref(),
            Phase::Boot,
            "phase_9_jailer_launch",
            { jail.launch(&api_socket).map_err(FcError::Jailer) }
        )?;
        let mut early_process_cleanup =
            Some(LaunchProcessCleanupGuard::from_jailed(&vm_id, &firecracker));

        // Phase 5b: create cgroup subtree now that we have live pids.
        let cgroup = diag_phase!(
            &mut diagnostics,
            &vm_id,
            request_id.as_deref(),
            Phase::HostPreflight,
            "phase_5b_cgroup_create",
            { phase_5b_cgroup_create(backend_config.cgroup_mode, &vm_id, &jail, &firecracker) }
        )?;
        let mut process_cleanup = early_process_cleanup
            .take()
            .expect("launch process cleanup guard must exist after firecracker spawn");

        // Phase 10: open UDS REST client (retries for up to 5 s).
        let host_api_socket =
            firecracker_api_socket_path(&run_dir, &backend_config.discovery.firecracker_bin);
        let client = diag_phase!(
            &mut diagnostics,
            &vm_id,
            request_id.as_deref(),
            Phase::Boot,
            "phase_10_open_uds",
            { phase_10_open_uds(&host_api_socket) }
        )?;

        // Phase 11: REST PUTs in documented order.
        diag_phase!(
            &mut diagnostics,
            &vm_id,
            request_id.as_deref(),
            Phase::Boot,
            "phase_11_rest_puts",
            {
                phase_11_rest_puts(
                    &client,
                    &storage,
                    &self.config,
                    &vm_id,
                    backend_config.discovery.manifest.image_kind,
                    backend_config.discovery.manifest.kernel_kind,
                )
            }
        )?;

        // Phase 11b: pre-create the inverted-readiness UnixListener at
        // `<jail>/vsock.sock_<READY_PORT_DEFAULT>`. Firecracker's muxer
        // connects to this path when the guest does outbound to the
        // ready port; if it doesn't exist when that happens, the muxer
        // RSTs the guest. Must be created before InstanceStart.
        let vsock_uds = vsock_socket_path(&run_dir, &backend_config.discovery.firecracker_bin);
        let ready_uds = ready_listener_path(&vsock_uds);
        let ready_listener = diag_phase!(
            &mut diagnostics,
            &vm_id,
            request_id.as_deref(),
            Phase::Ready,
            "phase_11b_ready_listener_bind",
            { phase_11b_bind_ready_listener(&ready_uds, backend_config.jail_uid) }
        )?;

        diag_phase!(
            &mut diagnostics,
            &vm_id,
            request_id.as_deref(),
            Phase::Boot,
            "phase_11c_boot_identity_record",
            { crate::boot_identity::record(&run_dir, &backend_config.discovery) }
        )?;

        // Phase 12a: InstanceStart.
        diag_phase!(
            &mut diagnostics,
            &vm_id,
            request_id.as_deref(),
            Phase::Boot,
            "phase_12a_instance_start",
            { client.instance_action(InstanceAction::InstanceStart) }
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
            &mut diagnostics,
            &vm_id,
            request_id.as_deref(),
            Phase::Ready,
            "phase_12b_ready_accept",
            { phase_12b_ready_accept(&ready_listener, &ready_uds, &vsock_uds, &vm_id) }
        )?;
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
        let watcher_stop = Arc::new(AtomicBool::new(false));
        let watcher_thread = self.config.idle_timeout.map(|timeout| {
            spawn_idle_watcher(
                timeout,
                vsock_uds.clone(),
                Arc::clone(&last_activity_ns),
                Arc::clone(&active_execs),
                Arc::clone(&idle_timed_out),
                Arc::clone(&watcher_stop),
                vm_id.clone(),
            )
        });

        let kill_guard = crate::types::ForceKillGuard::new(
            vm_id.clone(),
            firecracker.firecracker_pid,
            firecracker.jailer_pid,
            Arc::clone(&watcher_stop),
            None,
        );
        process_cleanup.disarm();
        run_dir_cleanup.disarm();
        Ok(RunningSandbox {
            vm_id,
            request_id,
            run_dir,
            jail,
            cgroup,
            rootfs: storage.rootfs,
            scratch: storage.scratch,
            snapshot_mount: None,
            client,
            firecracker,
            permit: self.permit,
            lease_guard,
            backend: self.backend,
            last_activity_ns,
            active_execs,
            idle_timed_out,
            watcher_stop,
            watcher_thread,
            diagnostics,
            preallocated_drive_slots: storage.preallocated_drive_slots.len() as u8,
            one_shot: self.config.one_shot,
            one_shot_consumed: false,
            kill_guard,
        })
    }
}

/// Retry cap for `phase_restore_probe_exec_channel`.
///
/// 5 s is generous; empirically the vsock TRANSPORT_RESET settles in < 1 s.
const RESTORE_PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// Sleep between probe attempts. Short so the common (fast) path is quick.
const RESTORE_PROBE_SLEEP: Duration = Duration::from_millis(50);

impl Sandbox {
    /// Restore a previously-captured snapshot into a running sandbox.
    ///
    /// Alternative to [`Sandbox::launch`] for the warm-pool path: instead of
    /// cold-booting, load from a snapshot pair and probe the exec channel to
    /// confirm guestd is live.
    ///
    /// # Phases
    ///
    /// 1. Allocate run-dir (`<run_root>/<vm_id>/`).
    /// 2. Jailer materialize — sets up the jail directory layout and bind
    ///    mounts (drives, kernel are NOT needed for restore; they are present
    ///    in the jail for layout compatibility, but Firecracker takes
    ///    drive/vsock state from the snapshot).
    /// 3. Spawn the Firecracker process via the jailer.
    /// 4. Open the UDS REST client.
    /// 5. Call `m80_snapshot::restore` with `resume: true` — this (a) removes
    ///    any stale `vsock.sock`, (b) issues PUT `/snapshot/load`, (c) issues
    ///    PATCH `/vm` Resumed.
    /// 6. Probe `CONNECT 9001` against the restored vsock UDS to confirm
    ///    guestd's exec listen socket is live (retry loop, 50 ms sleep, 5 s cap).
    ///
    /// The cold-boot inverted-readiness handshake (phases 11b / 12b) is
    /// **not used** on the restore path — guestd does not re-dial after
    /// TRANSPORT_RESET. The probe in step 6 is the only readiness signal.
    pub fn launch_from_snapshot(
        self,
        snapshot: SnapshotPaths,
        discovery: &Discovery,
    ) -> Result<RunningSandbox, FcError> {
        let vm_id = self.resolve_vm_id();
        let backend_config = &self.backend.config;
        let run_root = &backend_config.run_root;

        // Phase 1: run-root prep.
        let run_dir = phase("phase_1_run_root_prep", &vm_id, || {
            phase_1_run_root_prep(run_root, &vm_id)
        })?;
        let mut run_dir_cleanup = LaunchRunDirCleanupGuard::new(&vm_id, run_dir.clone());
        let request_id = self.config.request_id.clone();
        let mut diagnostics = crate::diagnostics::open(&run_dir, &vm_id, request_id.as_deref());

        // Phase 2: lease acquisition.
        let lease_guard = phase("phase_2_lease", &vm_id, || write_ownership_lock(&run_dir))?;

        // Phase 3: storage prep (overlay + optional scratch — still needed
        // for the jailer bind-mount layout even on restore path).
        let storage = diag_phase!(
            &mut diagnostics,
            &vm_id,
            request_id.as_deref(),
            Phase::StoragePrepare,
            "phase_3_storage_prep",
            {
                phase_3_storage_prep(
                    &vm_id,
                    &backend_config.discovery.rootfs,
                    &self.config,
                    &run_dir,
                )
            }
        )?;
        crate::diagnostics::record_owned(
            &mut diagnostics,
            Phase::StoragePrepare,
            &vm_id,
            request_id.as_deref(),
            "storage prepared for restore",
        );

        // Phase 4: jailer materialize.
        let jail = diag_phase!(
            &mut diagnostics,
            &vm_id,
            request_id.as_deref(),
            Phase::Boot,
            "phase_4_jailer_materialize",
            {
                phase_4_jailer_materialize(JailerMaterializeInput {
                    jailer_bin: &backend_config.discovery.jailer_bin,
                    jailer_harden_bin: &backend_config.discovery.jailer_harden_bin,
                    firecracker_bin: &backend_config.discovery.firecracker_bin,
                    uid: backend_config.jail_uid,
                    gid: backend_config.jail_gid,
                    run_dir: &run_dir,
                    kernel: &backend_config.discovery.kernel,
                    storage: &storage,
                    daemonize: self.config.daemonize,
                    netns_path: join_netns_path(&self.config.network),
                })
            }
        )?;

        // Phase 5: cgroup probe (restore path honours cgroup mode too).
        diag_phase!(
            &mut diagnostics,
            &vm_id,
            request_id.as_deref(),
            Phase::HostPreflight,
            "phase_5_cgroup_probe",
            { phase_5_cgroup_probe(backend_config.cgroup_mode) }
        )?;

        // Phase 8: compute the API socket path (inside the jail root).
        let api_socket =
            firecracker_api_socket_path(&run_dir, &backend_config.discovery.firecracker_bin);

        // Phase 9: spawn Firecracker via jailer.
        let firecracker = diag_phase!(
            &mut diagnostics,
            &vm_id,
            request_id.as_deref(),
            Phase::Boot,
            "phase_9_jailer_launch",
            { jail.launch(&api_socket).map_err(FcError::Jailer) }
        )?;
        let mut early_process_cleanup =
            Some(LaunchProcessCleanupGuard::from_jailed(&vm_id, &firecracker));

        // Phase 5b: create cgroup subtree.
        let cgroup = diag_phase!(
            &mut diagnostics,
            &vm_id,
            request_id.as_deref(),
            Phase::HostPreflight,
            "phase_5b_cgroup_create",
            { phase_5b_cgroup_create(backend_config.cgroup_mode, &vm_id, &jail, &firecracker) }
        )?;
        let mut process_cleanup = early_process_cleanup
            .take()
            .expect("restore process cleanup guard must exist after firecracker spawn");

        // Phase 10: open UDS REST client.
        let host_api_socket =
            firecracker_api_socket_path(&run_dir, &backend_config.discovery.firecracker_bin);
        let client = diag_phase!(
            &mut diagnostics,
            &vm_id,
            request_id.as_deref(),
            Phase::Boot,
            "phase_10_open_uds",
            { phase_10_open_uds(&host_api_socket) }
        )?;

        // Phase restore-load: remove stale vsock.sock + PUT /snapshot/load +
        // PATCH /vm Resumed (resume: true).
        let vsock_uds = vsock_socket_path(&run_dir, &backend_config.discovery.firecracker_bin);
        let snapshot_bind = diag_phase!(
            &mut diagnostics,
            &vm_id,
            request_id.as_deref(),
            Phase::Boot,
            "phase_restore_snapshot_bind",
            {
                bind_snapshot_parent_into_jail(
                    &jail.jail_path,
                    &snapshot,
                    backend_config.jail_uid,
                    backend_config.jail_gid,
                )
            }
        )?;
        diag_phase!(
            &mut diagnostics,
            &vm_id,
            request_id.as_deref(),
            Phase::Boot,
            "phase_restore_load",
            {
                snapshot_restore(RestoreRequest {
                    fc_socket: host_api_socket.clone(),
                    paths: snapshot_bind.paths.clone(),
                    vsock_uds: vsock_uds.clone(),
                    resume: true,
                })
                .map_err(FcError::Snapshot)
            }
        )?;
        crate::diagnostics::record_owned(
            &mut diagnostics,
            Phase::Boot,
            &vm_id,
            request_id.as_deref(),
            "snapshot restored",
        );

        // Phase restore-probe: CONNECT 9001 retry loop.
        // Replaces the cold-boot phase_12b_ready_accept.
        diag_phase!(
            &mut diagnostics,
            &vm_id,
            request_id.as_deref(),
            Phase::Ready,
            "phase_restore_probe_exec_channel",
            { phase_restore_probe_exec_channel(&vsock_uds, &vm_id) }
        )?;
        crate::diagnostics::record_owned(
            &mut diagnostics,
            Phase::Ready,
            &vm_id,
            request_id.as_deref(),
            "restored guestd ready",
        );

        let _ = discovery; // Discovery is passed for API symmetry; not needed beyond the phases above.

        let last_activity_ns = Arc::new(AtomicU64::new(monotonic_ns()));
        let active_execs = Arc::new(AtomicUsize::new(0));
        let idle_timed_out = Arc::new(AtomicBool::new(false));
        let watcher_stop = Arc::new(AtomicBool::new(false));
        let watcher_thread = self.config.idle_timeout.map(|timeout| {
            spawn_idle_watcher(
                timeout,
                vsock_uds.clone(),
                Arc::clone(&last_activity_ns),
                Arc::clone(&active_execs),
                Arc::clone(&idle_timed_out),
                Arc::clone(&watcher_stop),
                vm_id.clone(),
            )
        });

        let snapshot_mount = Some(snapshot_bind.into_mount_path());
        let kill_guard = crate::types::ForceKillGuard::new(
            vm_id.clone(),
            firecracker.firecracker_pid,
            firecracker.jailer_pid,
            Arc::clone(&watcher_stop),
            snapshot_mount.clone(),
        );
        process_cleanup.disarm();
        run_dir_cleanup.disarm();
        Ok(RunningSandbox {
            vm_id,
            request_id,
            run_dir,
            jail,
            cgroup,
            rootfs: storage.rootfs,
            scratch: storage.scratch,
            snapshot_mount,
            client,
            firecracker,
            permit: self.permit,
            lease_guard,
            backend: self.backend,
            last_activity_ns,
            active_execs,
            idle_timed_out,
            watcher_stop,
            watcher_thread,
            diagnostics,
            preallocated_drive_slots: storage.preallocated_drive_slots.len() as u8,
            one_shot: self.config.one_shot,
            one_shot_consumed: false,
            kill_guard,
        })
    }
}

/// Probe `CONNECT <GUEST_PORT_DEFAULT>` against the restored vsock UDS.
///
/// After `PATCH /vm Resumed`, Firecracker delivers the queued
/// `VIRTIO_VSOCK_EVENT_TRANSPORT_RESET` to the guest. The guest vsock
/// driver processes it and tears down established connections; vsock LISTEN
/// sockets (guestd's exec listener on port 9001) survive.
///
/// The probe races against TRANSPORT_RESET processing. On failure the muxer
/// returns `RST` / `EOF`; we sleep 50 ms and retry. A 5 s cap is safe:
/// empirically the settle time is < 1 s.
fn phase_restore_probe_exec_channel(vsock_uds: &Path, vm_id: &str) -> Result<Channel, FcError> {
    let deadline = Instant::now() + RESTORE_PROBE_TIMEOUT;
    let mut attempt = 0u32;
    loop {
        attempt += 1;
        match Channel::open_uds_only(vsock_uds, GUEST_PORT_DEFAULT) {
            Ok(channel) => {
                tracing::info!(vm_id, attempt, "restore probe: exec channel live");
                return Ok(channel);
            }
            Err(e) => {
                if Instant::now() >= deadline {
                    tracing::error!(
                        vm_id,
                        attempt,
                        error = %e,
                        "restore probe: exec channel not live after timeout"
                    );
                    return Err(FcError::GuestdReadyTimeout {
                        path: vsock_uds.to_path_buf(),
                        timeout: RESTORE_PROBE_TIMEOUT,
                    });
                }
                tracing::debug!(
                    vm_id,
                    attempt,
                    error = %e,
                    "restore probe: attempt failed, retrying"
                );
                std::thread::sleep(RESTORE_PROBE_SLEEP);
            }
        }
    }
}

/// Phase 1: create `<run_root>/<vm_id>/`.
///
/// `create_dir_all` is appropriate here: the m80 process owns the run_root
/// (verified by preflight), and the per-VM dir is a fresh creation, not a
/// silent recovery of existing state.
fn phase_1_run_root_prep(run_root: &Path, vm_id: &str) -> Result<PathBuf, FcError> {
    let run_dir = run_dir_path(run_root, vm_id);
    std::fs::create_dir_all(&run_dir)?;
    Ok(run_dir)
}

/// Phase 4: compute a `JailerConfig`, run `Plan::compute`, and materialize.
struct JailerMaterializeInput<'a> {
    jailer_bin: &'a Path,
    jailer_harden_bin: &'a Path,
    firecracker_bin: &'a Path,
    uid: u32,
    gid: u32,
    run_dir: &'a Path,
    kernel: &'a Path,
    storage: &'a StoragePrep,
    daemonize: bool,
    netns_path: Option<&'a Path>,
}

fn phase_4_jailer_materialize(
    input: JailerMaterializeInput<'_>,
) -> Result<m80_jailer::MaterializedJail, FcError> {
    let mut bindings = vec![
        // Kernel — read-only inside the jail.
        Binding {
            source: input.kernel.to_path_buf(),
            dest: PathBuf::from("kernel"),
            mode: BindMode::Ro,
        },
        // Shared read-only base ext4 (vda). Same host file across all VMs;
        // bind RO so the jail cannot mutate the shared image.
        Binding {
            source: input.storage.rootfs.base_path().to_path_buf(),
            dest: PathBuf::from("rootfs.ext4"),
            mode: BindMode::Ro,
        },
        // Per-VM sparse overlay ext4 (vdb). Writable; holds all guest writes.
        Binding {
            source: input.storage.rootfs.overlay_path().to_path_buf(),
            dest: PathBuf::from("rootfs.overlay.ext4"),
            mode: BindMode::Rw,
        },
    ];

    if let Some(scratch) = &input.storage.scratch {
        bindings.push(Binding {
            source: scratch.path().to_path_buf(),
            dest: PathBuf::from("scratch.ext4"),
            mode: BindMode::Rw,
        });
    }

    for (slot, path) in input.storage.preallocated_drive_slots.iter().enumerate() {
        bindings.push(Binding {
            source: path.clone(),
            dest: PathBuf::from(preallocated_drive_slot_filename(slot as u8)),
            mode: BindMode::Rw,
        });
    }

    let sockets = vec![JailerSocket::Firecracker, JailerSocket::Vsock];

    let jailer_config = JailerConfig {
        jailer_bin: input.jailer_bin.to_path_buf(),
        jailer_harden_bin: Some(input.jailer_harden_bin.to_path_buf()),
        firecracker_bin: input.firecracker_bin.to_path_buf(),
        run_dir: input.run_dir.to_path_buf(),
        uid: input.uid,
        gid: input.gid,
        bindings,
        sockets,
        resource_limits: m80_jailer::ResourceLimits::default(),
        new_pid_ns: false,
        daemonize: input.daemonize,
        new_cgroup_ns: false,
        netns_path: input.netns_path.map(Path::to_path_buf),
        stdio_log: Some(console_log_path(input.run_dir)),
    };

    let plan = Plan::compute(&jailer_config)?;
    plan.materialize().map_err(FcError::Jailer)
}

fn join_netns_path(policy: &crate::NetworkPolicy) -> Option<&Path> {
    match policy {
        crate::NetworkPolicy::JoinNetns { netns_path } => Some(netns_path.as_path()),
        crate::NetworkPolicy::NoEgress | crate::NetworkPolicy::AllowOutbound { .. } => None,
    }
}

/// Phase 5: probe that cgroup v2 is available when `UnifiedV2` mode is
/// requested. Actual subtree creation happens in phase 5b (after launch).
fn phase_5_cgroup_probe(mode: CgroupMode) -> Result<(), FcError> {
    match mode {
        CgroupMode::Disabled => Ok(()),
        CgroupMode::UnifiedV2 => match Subtree::probe() {
            Ok(()) => Ok(()),
            Err(m80_cgroup::CgroupError::UnsupportedHostMode) => {
                Err(FcError::Config(ConfigError::InvalidValue {
                    field: "cgroup_mode",
                    reason: "UnifiedV2 requested but host is not cgroup v2".into(),
                }))
            }
            Err(e) => Err(FcError::Config(ConfigError::Other(format!(
                "cgroup probe: {e}"
            )))),
        },
    }
}

/// Phase 5b (post-launch): create the cgroup subtree now that we have live
/// pids.
///
/// Cgroup v2 only lets a pid be moved into a leaf cgroup after the pid
/// exists; `Subtree::create` writes `firecracker_pid` to `cgroup.procs`,
/// which means we have to wait for `MaterializedJail::launch` (phase 9)
/// to return that pid before we can do this work. That's why "5b" is
/// out-of-order with the bare numbering — the README pipeline is "in the
/// order Firecracker requires it", not "in the source-line order".
fn phase_5b_cgroup_create(
    mode: CgroupMode,
    vm_id: &str,
    jail: &m80_jailer::MaterializedJail,
    jailed: &m80_jailer::JailedFirecracker,
) -> Result<Option<Subtree>, FcError> {
    match mode {
        CgroupMode::Disabled => Ok(None),
        CgroupMode::UnifiedV2 => {
            // FcError::Cgroup wraps CgroupError via #[from]; `?` does the
            // conversion so we keep the structured cause for the CLI's
            // error → exit-code map.
            let subtree = Subtree::create(vm_id, jail, jailed, &Limits::m80_default())?;
            Ok(Some(subtree))
        }
    }
}

/// Phase 6: resolve the network mode. Rejects `OutboundNat` (deferred to v0.2).
fn phase_6_network_realize(config: &SandboxConfig) -> Result<RealizedNetwork, FcError> {
    match m80_net_mode::resolve(&config.network) {
        VmNetworkMode::NoEgress => Ok(RealizedNetwork::NoEgress),
        VmNetworkMode::JoinNetns { netns_path } => Ok(RealizedNetwork::JoinNetns { netns_path }),
        VmNetworkMode::OutboundNat { .. } => Err(FcError::Config(ConfigError::Other(
            "OutboundNat networking is deferred to v0.2; use NetworkPolicy::NoEgress in v0.1"
                .into(),
        ))),
    }
}

/// Phase 10: open the Firecracker UDS REST client.
///
/// Retries for up to 5 s to allow Firecracker to create the socket after
/// jailer exec.
fn phase_10_open_uds(api_socket: &Path) -> Result<Client, FcError> {
    let deadline = Instant::now() + API_SOCKET_TIMEOUT;
    loop {
        if api_socket.exists() {
            match Client::new(api_socket) {
                Ok(client) => return Ok(client),
                Err(e) if Instant::now() < deadline => {
                    tracing::debug!(err = %e, "phase_10_open_uds: Client::new failed, retrying");
                }
                Err(e) => return Err(FcError::Client(e)),
            }
        }
        if Instant::now() >= deadline {
            return Err(FcError::ApiSocketTimeout {
                path: api_socket.to_path_buf(),
                timeout: API_SOCKET_TIMEOUT,
            });
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Phase 11: PUT all Firecracker resources in the documented order.
///
/// From Firecracker's perspective, resources must be PUT before `InstanceStart`:
/// machine-config → boot-source → drives (root first) → vsock.
#[allow(clippy::too_many_arguments)]
fn phase_11_rest_puts(
    client: &Client,
    storage: &StoragePrep,
    config: &SandboxConfig,
    vm_id: &str,
    image_kind: m80_image_manifest::ImageKind,
    kernel_kind: m80_image_manifest::KernelKind,
) -> Result<(), FcError> {
    let puts = plan_preboot_puts(
        config,
        vm_id,
        image_kind,
        kernel_kind,
        storage.scratch.is_some(),
    );
    apply_preboot_puts(client, &puts)
}

/// Phase 12b: poll the host vsock UDS until the in-VM guestd is ready.
///
/// # v0.1 simplification
///
/// Bind the inverted-readiness `UnixListener` at `<vsock_uds>_<READY_PORT>`.
///
/// Firecracker's muxer follows the `<host_sock_path>_<port>` convention
/// when the guest does an outbound vsock connect — it `UnixStream::connect()`s
/// to that path. We bind the listener BEFORE InstanceStart so the muxer
/// finds it ready when guestd's outbound connect lands.
///
/// Permissions: the file is created with the m80 process's umask. Since
/// Firecracker (running as `jail_uid` post-jailer-launch) needs to
/// `connect(2)` to the socket — which requires write permission on the
/// socket file — we explicitly chown to the jail uid.
fn phase_11b_bind_ready_listener(path: &Path, jail_uid: u32) -> Result<UnixListener, FcError> {
    if path.exists() {
        // Stale from a previous launch with the same vm_id. Remove so
        // bind() doesn't fail with EADDRINUSE.
        let _ = std::fs::remove_file(path);
    }
    let listener = UnixListener::bind(path).map_err(FcError::Io)?;

    // Make the socket connect()-able by the jail uid. Both the inode
    // ownership (chown) and the directory's access bits matter; the
    // jailer materialize step already produces a jail dir owned by
    // `jail_uid`, so `chown` on the socket file alone is sufficient.
    use nix::unistd::{chown, Uid};
    chown(path, Some(Uid::from_raw(jail_uid)), None)
        .map_err(|e| FcError::Io(std::io::Error::from_raw_os_error(e as i32)))?;

    Ok(listener)
}

fn ready_listener_path(vsock_uds: &Path) -> PathBuf {
    let mut path = vsock_uds.as_os_str().to_os_string();
    path.push(format!("_{READY_PORT_DEFAULT}"));
    PathBuf::from(path)
}

/// `accept()` the inverted-readiness signal from m80-guestd and validate the
/// protocol-version byte.
///
/// The host-local `accept()` poll-loop here is not a muxer-polling race.
/// We're polling our own UnixListener; the muxer only fires once (when guestd
/// does its outbound connect). No EAGAIN cascade.
fn phase_12b_ready_accept(
    ready_listener: &UnixListener,
    ready_path: &Path,
    _vsock_uds: &Path,
    vm_id: &str,
) -> Result<(), FcError> {
    accept_ready_signal(ready_listener, ready_path, READY_TIMEOUT)?;
    tracing::info!(vm_id, "ready signal received from guestd");
    Ok(())
}

fn accept_ready_signal(
    ready_listener: &UnixListener,
    ready_path: &Path,
    timeout: Duration,
) -> Result<(), FcError> {
    ready_listener.set_nonblocking(true).map_err(FcError::Io)?;
    let deadline = Instant::now() + timeout;

    let mut stream = loop {
        // Check deadline before sleeping to avoid overshooting by one poll interval.
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(FcError::GuestdReadyTimeout {
                path: ready_path.to_path_buf(),
                timeout,
            });
        }
        match ready_listener.accept() {
            Ok((s, _)) => break s,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(remaining.min(READY_ACCEPT_POLL));
            }
            Err(e) => return Err(FcError::Io(e)),
        }
    };

    stream
        .set_read_timeout(Some(READY_VERSION_READ_TIMEOUT))
        .map_err(FcError::Io)?;
    let mut buf = [0u8; 1];
    stream.read_exact(&mut buf).map_err(FcError::Io)?;
    if buf[0] != m80_proto::PROTOCOL_VERSION as u8 {
        return Err(FcError::Protocol(WireProtocolError::UnsupportedVersion {
            expected: m80_proto::PROTOCOL_VERSION,
            got: u32::from(buf[0]),
        }));
    }
    drop(stream);
    Ok(())
}

#[cfg(test)]
mod tests;
