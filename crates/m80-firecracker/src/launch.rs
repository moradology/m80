//! [`Sandbox::launch`] and the 12-phase preboot pipeline.
//!
//! # v0.1 simplifications
//!
//! - **Phase 12b (ready accept)**: m80-guestd connects out to the host on
//!   `m80_proto::READY_PORT_DEFAULT` immediately after binding its
//!   listener; the host pre-creates a `UnixListener` at
//!   `<vsock_uds>_<READY_PORT>` (Phase 11b) and `accept()`s. Event-driven,
//!   no polling, no muxer EAGAIN race.

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::os::unix::prelude::AsFd;
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
use m80_proto::GUEST_PORT_DEFAULT;
use m80_proto::{
    Envelope, ExecExit, ExecRequest, RawEnvelope, PAYLOAD_KIND_EXEC_EXIT, PAYLOAD_KIND_EXEC_STDERR,
    PAYLOAD_KIND_EXEC_STDOUT,
};
use m80_snapshot::{
    restore as snapshot_restore, restore_preverified as snapshot_restore_preverified,
    RestoreRequest, SnapshotPaths,
};
use m80_snapshot_template::PinnedTemplate;
use m80_vsock::{Channel, VsockError};
use nix::errno::Errno;
use nix::poll::{poll, PollFd, PollFlags, PollTimeout};

use crate::diagnostics::phase;
use crate::error::{ConfigError, FcError, WireProtocolError};
use crate::layout::{
    console_log_path, firecracker_api_socket_path, pmem_layer_jail_bind_dest, pmem_layer_jail_path,
    preallocated_drive_slot_filename, run_dir_path, vsock_socket_path,
};
use crate::lifecycle::{
    monotonic_ns, phase_13_pmem_guest_mount, phase_restore_post_restore_hooks,
    prepare_snapshot_paths, prepare_template_snapshot_paths, snapshot_stage_parent,
    spawn_idle_watcher, SNAPSHOT_BIND_DEST,
};
use crate::pmem::{validate_pmem_layers, PmemSharing};
use crate::preboot::{apply_preboot_puts, plan_preboot_puts};
use crate::runroot::write_ownership_lock;
use crate::storage_prep::phase_3_storage_prep;
use crate::types::{
    CgroupMode, RealizedNetwork, RunningSandbox, Sandbox, SandboxConfig, StoragePrep,
};
use crate::warm_pool::HookSpecSet;

mod failure_cleanup;
mod ready;
mod snapshot_prime;

use failure_cleanup::{
    LaunchNetworkCleanupGuard, LaunchProcessCleanupGuard, LaunchRunDirCleanupGuard,
};
use ready::{phase_11b_bind_ready_listener, phase_12b_ready_accept, ready_listener_path};
use snapshot_prime::prime_snapshot_files;

const RUN_DIR_MODE: u32 = 0o700;
const FIRECRACKER_SECCOMP_FILTER_JAIL_PATH: &str = "firecracker-seccomp-filter.bin";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SnapshotRestoreVerification {
    VerifyManifest,
    PreverifiedTemplate,
}

/// Record a diagnostics-annotated phase result.
///
/// Takes `diagnostics` (`&mut Option<Diagnostics>`), `vm_id` (`&str`), and
/// `request_id` (`Option<&str>`) as explicit arguments so the macro can be
/// defined at module scope — Rust `macro_rules!` cannot capture local
/// variables from the caller's scope.
macro_rules! diag_phase {
    ($current_phase:ident, $diag:expr, $vid:expr, $rid:expr, $phase:expr, $name:literal, $body:expr) => {{
        $current_phase = $name;
        crate::diagnostics::phase_result($diag, $phase, $name, $vid, $rid, || $body)
    }};
}

const API_SOCKET_TIMEOUT: Duration = Duration::from_secs(5);
#[cfg(debug_assertions)]
const FAIL_CGROUP_CREATE_FOR_VM_ENV: &str = "M80_TEST_FAIL_CGROUP_CREATE_FOR_VM";

impl Sandbox {
    /// Standalone constructor for callers without a `Backend`.
    ///
    /// Deferred to v0.2. The admission semaphore is bypassed in this path,
    /// but constructing a minimal Backend without a Discovery from preflight
    /// is not supported in v0.1. Use `Backend::admit` instead.
    pub fn new(_config: SandboxConfig) -> Result<Sandbox, FcError> {
        Err(FcError::UnsupportedOperation {
            operation: "Sandbox::new",
            reason: "v0.1 requires Backend::admit(config).launch()".into(),
        })
    }

    /// Resolve or auto-generate the VM identifier.
    fn resolve_vm_id(&self) -> String {
        self.config
            .vm_id
            .clone()
            .expect("Backend::admit stores the selected vm_id")
    }

    /// Delete the partial run directory if launch fails.
    ///
    /// The default is to preserve failed launch run directories under
    /// `.preserved/` so `failure_summary.json`, diagnostics, and console logs
    /// remain available for triage.
    #[must_use]
    pub fn delete_run_dir_on_launch_error(mut self) -> Self {
        self.delete_run_dir_on_launch_error = true;
        self
    }

    /// Run the strict 12-phase preboot pipeline (Created → Running).
    ///
    /// Consumes `self` so a failed launch cannot be retried — the admission
    /// permit is dropped on any error path.
    pub fn launch(self) -> Result<RunningSandbox, FcError> {
        let vm_id = self.resolve_vm_id();
        let backend = Arc::clone(&self.backend);
        let backend_for_running = Arc::clone(&backend);
        let backend_config = &backend.config;
        let run_root = &backend_config.run_root;
        validate_declared_pmem_layers(&self.config)?;

        // Phase 1: run-root prep.
        let run_dir = phase_1_run_root_prep_with_failure_artifact(
            run_root,
            &vm_id,
            self.config.request_id.as_deref(),
            self.delete_run_dir_on_launch_error,
        )?;
        let mut run_dir_cleanup = LaunchRunDirCleanupGuard::new(
            &vm_id,
            run_dir.clone(),
            self.delete_run_dir_on_launch_error,
        );
        let request_id = self.config.request_id.clone();
        let mut diagnostics = crate::diagnostics::open(&run_dir, &vm_id, request_id.as_deref());
        let summary_run_dir = run_dir.clone();
        let summary_vm_id = vm_id.clone();
        let summary_request_id = request_id.clone();
        let mut current_phase: &'static str = "phase_2_lease";
        let result = (|| -> Result<RunningSandbox, FcError> {
            // Phase 2: lease acquisition. Keep the guard inside RunningSandbox so
            // ownership.lock covers the whole VM lifetime, not just launch.
            let lease_guard = phase("phase_2_lease", &vm_id, || write_ownership_lock(&run_dir))?;

            // Phase 3: storage prep. Artifact sha256 verification is owned by
            // m80-preflight before Backend construction, not by each launch.
            let storage = diag_phase!(
                current_phase,
                &mut diagnostics,
                &vm_id,
                request_id.as_deref(),
                Phase::StoragePrepare,
                "phase_3_storage_prep",
                {
                    phase_3_storage_prep(
                        &vm_id,
                        &backend_config.discovery.pinned_rootfs.proc_fd_path(),
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

            let cold_launch_netns_path =
                cold_launch_netns_path(&self.config.network, run_root, &vm_id);

            // Phase 4: jailer materialize.
            let jail = diag_phase!(
                current_phase,
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
                        firecracker_seccomp_filter: &backend_config
                            .discovery
                            .firecracker_seccomp_filter,
                        uid: backend_config.jail_uid,
                        gid: backend_config.jail_gid,
                        run_dir: &run_dir,
                        kernel: &backend_config.discovery.kernel,
                        storage: &storage,
                        daemonize: self.config.daemonize,
                        netns_path: cold_launch_netns_path.as_deref(),
                        private_netns: private_vmm_netns(&self.config.network),
                        snapshot_parent: None,
                        snapshot_bind_mode: BindMode::Rw,
                    })
                }
            )?;
            // Phase 5: probe cgroup availability (creation happens after phase 9).
            diag_phase!(
                current_phase,
                &mut diagnostics,
                &vm_id,
                request_id.as_deref(),
                Phase::HostPreflight,
                "phase_5_cgroup_probe",
                { phase_5_cgroup_probe(backend_config.cgroup_mode) }
            )?;

            // Phase 6: resolve network mode.
            let net = diag_phase!(
                current_phase,
                &mut diagnostics,
                &vm_id,
                request_id.as_deref(),
                Phase::NetworkPrepare,
                "phase_6_network_realize",
                {
                    phase_6_network_realize(
                        &backend.network_helper,
                        &self.config,
                        &vm_id,
                        run_root,
                        &run_dir,
                    )
                }
            )?;
            let mut network_cleanup = match &net {
                RealizedNetwork::OutboundNat { .. } => Some(LaunchNetworkCleanupGuard::new(
                    &backend.network_helper,
                    &vm_id,
                    run_root.clone(),
                )),
                RealizedNetwork::NoEgress | RealizedNetwork::JoinNetns { .. } => None,
            };
            let network_message = match &net {
                RealizedNetwork::NoEgress => "network prepared".to_owned(),
                RealizedNetwork::OutboundNat {
                    tap_name,
                    vmm_netns_path,
                    ..
                } => {
                    format!(
                        "network prepared: outbound_nat tap {tap_name} netns {}",
                        vmm_netns_path.display()
                    )
                }
                RealizedNetwork::JoinNetns { netns_path, .. } => {
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

            // Phase 7: for OutboundNat, prepare PID-1 guest network tokens and
            // apply the host firewall/NAT policy before the VM can boot.
            let network_boot_args = diag_phase!(
                current_phase,
                &mut diagnostics,
                &vm_id,
                request_id.as_deref(),
                Phase::NetworkPrepare,
                "phase_7_outbound_guest_config",
                { phase_7_outbound_guest_config(&backend.network_helper, &net, &run_dir) }
            )?;

            // Phase 8: compute the API socket path (inside the jail root).
            let api_socket =
                firecracker_api_socket_path(&run_dir, &backend_config.discovery.firecracker_bin);

            // Phase 9: jailer exec's firecracker. Returns live pids.
            let firecracker = diag_phase!(
                current_phase,
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
                current_phase,
                &mut diagnostics,
                &vm_id,
                request_id.as_deref(),
                Phase::HostPreflight,
                "phase_5b_cgroup_create",
                {
                    phase_5b_cgroup_create(
                        backend_config.cgroup_mode,
                        &vm_id,
                        &self.config,
                        &jail,
                        &firecracker,
                    )
                }
            )?;
            let mut process_cleanup = early_process_cleanup
                .take()
                .expect("launch process cleanup guard must exist after firecracker spawn");

            // Phase 10: open UDS REST client (retries for up to 5 s).
            let client = diag_phase!(
                current_phase,
                &mut diagnostics,
                &vm_id,
                request_id.as_deref(),
                Phase::Boot,
                "phase_10_open_uds",
                { phase_10_open_uds(&api_socket) }
            )?;

            // Phase 11: REST PUTs in documented order.
            diag_phase!(
                current_phase,
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
                        backend_config.discovery.manifest.rootfs_format,
                        &net,
                        &network_boot_args,
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
                current_phase,
                &mut diagnostics,
                &vm_id,
                request_id.as_deref(),
                Phase::Ready,
                "phase_11b_ready_listener_bind",
                { phase_11b_bind_ready_listener(&ready_uds, backend_config.jail_uid) }
            )?;

            diag_phase!(
                current_phase,
                &mut diagnostics,
                &vm_id,
                request_id.as_deref(),
                Phase::Boot,
                "phase_11c_boot_identity_record",
                { crate::boot_identity::record(&run_dir, &backend_config.discovery) }
            )?;

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
                        &self.config.pmem_layers,
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
            let watcher_stop = Arc::new(AtomicBool::new(false));
            let watcher_thread = self.config.idle_timeout.map(|timeout| {
                spawn_idle_watcher(
                    timeout,
                    vsock_uds.clone(),
                    firecracker.firecracker_pid(),
                    Arc::clone(&last_activity_ns),
                    Arc::clone(&active_execs),
                    Arc::clone(&idle_timed_out),
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
            process_cleanup.disarm();
            let network_cleanup_enabled = network_cleanup.is_some();
            if let Some(guard) = &mut network_cleanup {
                guard.disarm();
            }
            run_dir_cleanup.disarm();
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
                permit: self.permit,
                lease_guard,
                backend: backend_for_running,
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
}

/// Retry cap for `phase_restore_probe_exec_channel`.
///
/// 5 s is generous; empirically the vsock TRANSPORT_RESET settles in < 1 s.
const RESTORE_PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// Sleep between probe attempts. Short so the common (fast) path is quick.
const RESTORE_PROBE_SLEEP: Duration = Duration::from_millis(50);

impl Sandbox {
    /// Restore a previously-captured snapshot without post-restore hooks.
    ///
    /// This preserves the existing direct snapshot restore contract. Warm-pool
    /// template restores that need entropy reseed or lease-specific identity
    /// work use [`Sandbox::launch_from_snapshot_with_hooks`].
    pub fn launch_from_snapshot(
        self,
        snapshot: SnapshotPaths,
        discovery: &Discovery,
    ) -> Result<RunningSandbox, FcError> {
        self.launch_from_snapshot_with_hooks(snapshot, discovery, HookSpecSet::empty())
    }

    /// Restore a previously-captured snapshot into a running sandbox.
    ///
    /// Alternative to [`Sandbox::launch`] for the warm-pool path: instead of
    /// cold-booting, load from a snapshot pair, probe the exec channel to
    /// confirm guestd is live, then send a host-driven post-restore hook
    /// request before returning the running VM to the caller.
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
    /// 6. Send a lightweight exec readiness probe over the restored vsock UDS
    ///    to confirm guestd itself can read, execute, and reply (retry loop,
    ///    50 ms sleep, 5 s cap).
    /// 7. If hooks were supplied, send `PostRestoreHookRequest`; guestd mixes
    ///    the host restore nonce, reseeds the guest CRNG, runs the hooks in
    ///    order, and returns a success/failure ack before this method returns.
    ///
    /// The cold-boot inverted-readiness handshake (phases 11b / 12b) is
    /// **not used** on the restore path — guestd does not re-dial after
    /// TRANSPORT_RESET. The probe in step 6 is the readiness signal; post-restore
    /// hooks are the lease-handoff gate.
    pub fn launch_from_snapshot_with_hooks(
        self,
        snapshot: SnapshotPaths,
        discovery: &Discovery,
        hooks: HookSpecSet,
    ) -> Result<RunningSandbox, FcError> {
        let snapshot_plan =
            prepare_snapshot_paths(&snapshot, &self.backend.config.run_root, false)?;
        self.launch_from_prepared_snapshot_with_hooks(
            snapshot,
            snapshot_plan,
            discovery,
            hooks,
            SnapshotRestoreVerification::VerifyManifest,
        )
    }

    pub(crate) fn launch_from_template_body_with_hooks(
        self,
        template: &PinnedTemplate,
        discovery: &Discovery,
        hooks: HookSpecSet,
    ) -> Result<RunningSandbox, FcError> {
        let snapshot = template.body_paths().snapshot_paths();
        let snapshot_plan = prepare_template_snapshot_paths(&snapshot)?;
        self.launch_from_prepared_snapshot_with_hooks(
            snapshot,
            snapshot_plan,
            discovery,
            hooks,
            SnapshotRestoreVerification::PreverifiedTemplate,
        )
    }

    fn launch_from_prepared_snapshot_with_hooks(
        self,
        snapshot: SnapshotPaths,
        snapshot_plan: crate::lifecycle::PreparedSnapshotPaths,
        discovery: &Discovery,
        hooks: HookSpecSet,
        verification: SnapshotRestoreVerification,
    ) -> Result<RunningSandbox, FcError> {
        let vm_id = self.resolve_vm_id();
        let backend = Arc::clone(&self.backend);
        let backend_for_running = Arc::clone(&backend);
        let backend_config = &backend.config;
        let run_root = &backend_config.run_root;
        validate_declared_pmem_layers(&self.config)?;

        // Phase 1: run-root prep.
        let run_dir = phase_1_run_root_prep_with_failure_artifact(
            run_root,
            &vm_id,
            self.config.request_id.as_deref(),
            self.delete_run_dir_on_launch_error,
        )?;
        let mut run_dir_cleanup = LaunchRunDirCleanupGuard::new(
            &vm_id,
            run_dir.clone(),
            self.delete_run_dir_on_launch_error,
        );
        let request_id = self.config.request_id.clone();
        let mut diagnostics = crate::diagnostics::open(&run_dir, &vm_id, request_id.as_deref());
        let summary_run_dir = run_dir.clone();
        let summary_vm_id = vm_id.clone();
        let summary_request_id = request_id.clone();
        let mut current_phase: &'static str = "phase_2_lease";
        let result = (|| -> Result<RunningSandbox, FcError> {
            // Phase 2: lease acquisition.
            let lease_guard = phase("phase_2_lease", &vm_id, || write_ownership_lock(&run_dir))?;

            // Phase 3: storage prep (overlay + optional scratch — still needed
            // for the jailer bind-mount layout even on restore path).
            let storage = diag_phase!(
                current_phase,
                &mut diagnostics,
                &vm_id,
                request_id.as_deref(),
                Phase::StoragePrepare,
                "phase_3_storage_prep",
                {
                    phase_3_storage_prep(
                        &vm_id,
                        &backend_config.discovery.pinned_rootfs.proc_fd_path(),
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
                current_phase,
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
                        firecracker_seccomp_filter: &backend_config
                            .discovery
                            .firecracker_seccomp_filter,
                        uid: backend_config.jail_uid,
                        gid: backend_config.jail_gid,
                        run_dir: &run_dir,
                        kernel: &backend_config.discovery.kernel,
                        storage: &storage,
                        daemonize: self.config.daemonize,
                        netns_path: join_netns_path(&self.config.network),
                        private_netns: private_vmm_netns(&self.config.network),
                        snapshot_parent: Some(snapshot_plan.host_parent.as_path()),
                        snapshot_bind_mode: BindMode::Ro,
                    })
                }
            )?;
            // Phase 5: cgroup probe (restore path honours cgroup mode too).
            diag_phase!(
                current_phase,
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
                current_phase,
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
                current_phase,
                &mut diagnostics,
                &vm_id,
                request_id.as_deref(),
                Phase::HostPreflight,
                "phase_5b_cgroup_create",
                {
                    phase_5b_cgroup_create(
                        backend_config.cgroup_mode,
                        &vm_id,
                        &self.config,
                        &jail,
                        &firecracker,
                    )
                }
            )?;
            let mut process_cleanup = early_process_cleanup
                .take()
                .expect("restore process cleanup guard must exist after firecracker spawn");

            // Phase 10: open UDS REST client.
            let client = diag_phase!(
                current_phase,
                &mut diagnostics,
                &vm_id,
                request_id.as_deref(),
                Phase::Boot,
                "phase_10_open_uds",
                { phase_10_open_uds(&api_socket) }
            )?;

            // Phase restore-prime: queue host readahead on the real snapshot
            // files before translating them into jail-visible /snapshot paths.
            diag_phase!(
                current_phase,
                &mut diagnostics,
                &vm_id,
                request_id.as_deref(),
                Phase::Boot,
                "phase_restore_snapshot_prime",
                { prime_snapshot_files(&snapshot) }
            )?;

            // Phase restore-load: remove stale vsock.sock + PUT /snapshot/load +
            // PATCH /vm Resumed (resume: true).
            let vsock_uds = vsock_socket_path(&run_dir, &backend_config.discovery.firecracker_bin);
            let snapshot_bind = diag_phase!(
                current_phase,
                &mut diagnostics,
                &vm_id,
                request_id.as_deref(),
                Phase::Boot,
                "phase_restore_snapshot_bind",
                { Ok::<_, FcError>(snapshot_plan.clone()) }
            )?;
            diag_phase!(
                current_phase,
                &mut diagnostics,
                &vm_id,
                request_id.as_deref(),
                Phase::Boot,
                "phase_restore_load",
                {
                    let req = RestoreRequest {
                        api_socket: api_socket.clone(),
                        paths: snapshot_bind.jail_paths.clone(),
                        host_paths: snapshot.clone(),
                        expected_firecracker_version: discovery
                            .manifest
                            .expected_firecracker_version
                            .clone(),
                        vsock_uds: vsock_uds.clone(),
                        enable_diff_snapshots: false,
                        resume: true,
                    };
                    match verification {
                        SnapshotRestoreVerification::VerifyManifest => snapshot_restore(req),
                        SnapshotRestoreVerification::PreverifiedTemplate => {
                            snapshot_restore_preverified(req)
                        }
                    }
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

            // Phase restore-probe: exec round-trip retry loop.
            // Replaces the cold-boot phase_12b_ready_accept.
            diag_phase!(
                current_phase,
                &mut diagnostics,
                &vm_id,
                request_id.as_deref(),
                Phase::Ready,
                "phase_restore_probe_exec_channel",
                { phase_restore_probe_exec_channel(&vsock_uds, &vm_id) }
            )?;
            diag_phase!(
                current_phase,
                &mut diagnostics,
                &vm_id,
                request_id.as_deref(),
                Phase::Ready,
                "phase_restore_post_restore_hooks",
                {
                    phase_restore_post_restore_hooks(
                        &vsock_uds,
                        &vm_id,
                        request_id.as_deref(),
                        firecracker.firecracker_pid(),
                        &hooks,
                    )
                }
            )?;
            crate::diagnostics::record_owned(
                &mut diagnostics,
                Phase::Ready,
                &vm_id,
                request_id.as_deref(),
                "restored guestd ready",
            );
            let last_activity_ns = Arc::new(AtomicU64::new(monotonic_ns()));
            let active_execs = Arc::new(AtomicUsize::new(0));
            let idle_timed_out = Arc::new(AtomicBool::new(false));
            let watcher_stop = Arc::new(AtomicBool::new(false));
            let watcher_thread = self.config.idle_timeout.map(|timeout| {
                spawn_idle_watcher(
                    timeout,
                    vsock_uds.clone(),
                    firecracker.firecracker_pid(),
                    Arc::clone(&last_activity_ns),
                    Arc::clone(&active_execs),
                    Arc::clone(&idle_timed_out),
                    Arc::clone(&watcher_stop),
                    vm_id.clone(),
                )
            });

            let snapshot_mount = None;
            let kill_guard = crate::types::ForceKillGuard::new(
                vm_id.clone(),
                firecracker.firecracker_pid(),
                firecracker.jailer_pid(),
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
                shared_pmem_refs: storage.shared_pmem_refs,
                cgroup,
                rootfs: storage.rootfs,
                scratch: storage.scratch,
                snapshot_mount,
                client,
                firecracker,
                permit: self.permit,
                lease_guard,
                backend: backend_for_running,
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
                network_cleanup: false,
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
}

/// Probe guestd over `<GUEST_PORT_DEFAULT>` against the restored vsock UDS.
///
/// After `PATCH /vm Resumed`, Firecracker delivers the queued
/// `VIRTIO_VSOCK_EVENT_TRANSPORT_RESET` to the guest. The guest vsock
/// driver processes it and tears down established connections; vsock LISTEN
/// sockets (guestd's exec listener on port 9001) survive. A bare CONNECT is
/// not enough to prove guestd is scheduled and reading, because the kernel can
/// accept the socket while guestd is stopped. The probe therefore sends a tiny
/// exec request and waits for the terminal `exec_exit` frame.
///
/// The probe races against TRANSPORT_RESET processing. On failure the muxer can
/// return `RST` / `EOF`, or the opened channel can fail to make request/response
/// progress; we sleep 50 ms and retry. A 5 s cap is safe: empirically the
/// settle time is < 1 s.
fn phase_restore_probe_exec_channel(vsock_uds: &Path, vm_id: &str) -> Result<(), FcError> {
    let deadline = Instant::now() + RESTORE_PROBE_TIMEOUT;
    let mut attempt = 0u32;
    loop {
        attempt += 1;
        match try_restore_exec_probe(vsock_uds, vm_id, attempt) {
            Ok(()) => {
                tracing::info!(vm_id, attempt, "restore probe: guestd exec round-trip live");
                return Ok(());
            }
            Err(FcError::Vsock(e)) => {
                if Instant::now() >= deadline {
                    tracing::error!(
                        vm_id,
                        attempt,
                        error = %e,
                        "restore probe: guestd exec round-trip not live after timeout"
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
                    "restore probe: vsock attempt failed, retrying"
                );
                std::thread::sleep(RESTORE_PROBE_SLEEP);
            }
            Err(e) => return Err(e),
        }
    }
}

fn try_restore_exec_probe(vsock_uds: &Path, vm_id: &str, attempt: u32) -> Result<(), FcError> {
    let request_id = format!("{vm_id}-restore-probe-{attempt}");
    let req = ExecRequest {
        program: "/bin/true".into(),
        args: Vec::new(),
        cwd: None,
        env: None,
        stdin: None,
        timeout_ms: Some(1_000),
        streaming: true,
    };
    let envelope = Envelope::with_request_id(req, request_id.clone());
    let mut channel = Channel::open_uds_only(vsock_uds, GUEST_PORT_DEFAULT)?;
    channel.send(&envelope)?;

    loop {
        let frame = channel.recv_raw()?;
        check_restore_probe_request_id(&frame, &request_id)?;
        let kind = frame.kind.clone();
        match kind.as_str() {
            PAYLOAD_KIND_EXEC_STDOUT | PAYLOAD_KIND_EXEC_STDERR => {}
            PAYLOAD_KIND_EXEC_EXIT => {
                let _exit: m80_proto::Envelope<ExecExit> = frame
                    .decode()
                    .map_err(|e| FcError::Vsock(VsockError::Proto(e)))?;
                return Ok(());
            }
            _ => {
                return Err(FcError::Protocol(WireProtocolError::UnexpectedFrame {
                    context: "restore ready probe",
                    expected: "exec_stdout|exec_stderr|exec_exit",
                    got: kind,
                }));
            }
        }
    }
}

fn check_restore_probe_request_id(frame: &RawEnvelope, request_id: &str) -> Result<(), FcError> {
    if frame.request_id.as_deref() == Some(request_id) {
        return Ok(());
    }
    Err(FcError::Protocol(WireProtocolError::RequestIdMismatch {
        context: "restore ready probe",
        expected: request_id.to_owned(),
        got: frame.request_id.clone(),
    }))
}

fn validate_declared_pmem_layers(config: &SandboxConfig) -> Result<(), FcError> {
    validate_pmem_layers(&config.pmem_layers)
}

/// Phase 1: create `<run_root>/<vm_id>/`.
///
/// `create_dir_all` is appropriate here: the m80 process owns the run_root
/// (verified by preflight), and the per-VM dir is a fresh creation, not a
/// silent recovery of existing state.
fn phase_1_run_root_prep(run_root: &Path, vm_id: &str) -> Result<PathBuf, FcError> {
    let run_dir = run_dir_path(run_root, vm_id);
    fs::DirBuilder::new()
        .recursive(true)
        .mode(RUN_DIR_MODE)
        .create(&run_dir)
        .map_err(|source| path_io(&run_dir, source))?;
    fs::set_permissions(&run_dir, fs::Permissions::from_mode(RUN_DIR_MODE))
        .map_err(|source| path_io(&run_dir, source))?;
    Ok(run_dir)
}

fn phase_1_run_root_prep_with_failure_artifact(
    run_root: &Path,
    vm_id: &str,
    request_id: Option<&str>,
    delete_on_error: bool,
) -> Result<PathBuf, FcError> {
    match phase("phase_1_run_root_prep", vm_id, || {
        phase_1_run_root_prep(run_root, vm_id)
    }) {
        Ok(run_dir) => Ok(run_dir),
        Err(err) => {
            let run_dir = run_dir_path(run_root, vm_id);
            preserve_launch_failure_artifact_if_run_dir_exists(
                &run_dir,
                vm_id,
                "phase_1_run_root_prep",
                request_id,
                &err,
                delete_on_error,
            );
            Err(err)
        }
    }
}

fn preserve_launch_failure_artifact_if_run_dir_exists(
    run_dir: &Path,
    vm_id: &str,
    failed_phase: &'static str,
    request_id: Option<&str>,
    err: &FcError,
    delete_on_error: bool,
) {
    if !run_dir.is_dir() {
        return;
    }
    crate::diagnostics::record_failure_summary_best_effort(
        run_dir,
        vm_id,
        failed_phase,
        request_id,
        err,
    );
    drop(LaunchRunDirCleanupGuard::new(
        vm_id,
        run_dir.to_path_buf(),
        delete_on_error,
    ));
}

/// Phase 4: compute a `JailerConfig`, run `Plan::compute`, and materialize.
struct JailerMaterializeInput<'a> {
    jailer_bin: &'a Path,
    jailer_harden_bin: &'a Path,
    firecracker_bin: &'a Path,
    firecracker_seccomp_filter: &'a Path,
    uid: u32,
    gid: u32,
    run_dir: &'a Path,
    kernel: &'a Path,
    storage: &'a StoragePrep,
    daemonize: bool,
    netns_path: Option<&'a Path>,
    private_netns: bool,
    snapshot_parent: Option<&'a Path>,
    snapshot_bind_mode: BindMode,
}

struct JailerLaunchConfigInput<'a> {
    jailer_bin: &'a Path,
    jailer_harden_bin: &'a Path,
    firecracker_bin: &'a Path,
    uid: u32,
    gid: u32,
    run_dir: &'a Path,
    daemonize: bool,
    netns_path: Option<&'a Path>,
    private_netns: bool,
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
        Binding {
            source: input.firecracker_seccomp_filter.to_path_buf(),
            dest: PathBuf::from(FIRECRACKER_SECCOMP_FILTER_JAIL_PATH),
            mode: BindMode::Ro,
        },
    ];
    let snapshot_parent = match input.snapshot_parent {
        Some(parent) => parent.to_path_buf(),
        None => {
            let stage_parent = snapshot_stage_parent(input.run_dir);
            fs::create_dir_all(&stage_parent).map_err(|source| FcError::PathIo {
                path: stage_parent.clone(),
                source,
            })?;
            stage_parent
        }
    };
    push_snapshot_bindings(&mut bindings, snapshot_parent, input.snapshot_bind_mode);

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
    push_pmem_backing_bindings(&mut bindings, &input.storage.pmem_backings);

    let sockets = vec![JailerSocket::Firecracker, JailerSocket::Vsock];

    let jailer_config = build_jailer_launch_config(
        JailerLaunchConfigInput {
            jailer_bin: input.jailer_bin,
            jailer_harden_bin: input.jailer_harden_bin,
            firecracker_bin: input.firecracker_bin,
            uid: input.uid,
            gid: input.gid,
            run_dir: input.run_dir,
            daemonize: input.daemonize,
            netns_path: input.netns_path,
            private_netns: input.private_netns,
        },
        bindings,
        sockets,
    );

    let plan = Plan::compute(&jailer_config)?;
    plan.materialize().map_err(FcError::Jailer)
}

fn push_pmem_backing_bindings(
    bindings: &mut Vec<Binding>,
    backings: &[crate::types::ResolvedPmemBacking],
) {
    for (slot, backing) in backings.iter().enumerate() {
        let dest = pmem_layer_jail_bind_dest(slot);
        debug_assert_eq!(dest, PathBuf::from(&backing.jail_basename));
        debug_assert_eq!(
            pmem_layer_jail_path(slot),
            PathBuf::from(format!("/{}", backing.jail_basename))
        );
        bindings.push(Binding {
            source: backing.host_path.clone(),
            dest,
            mode: match backing.sharing {
                PmemSharing::PerVm => BindMode::Ro,
                PmemSharing::Shared(_) => BindMode::RoImageStore,
            },
        });
    }
}

fn push_snapshot_bindings(bindings: &mut Vec<Binding>, source: PathBuf, mode: BindMode) {
    bindings.push(Binding {
        source: PathBuf::new(),
        dest: PathBuf::from(SNAPSHOT_BIND_DEST),
        mode: BindMode::CreateInsideJail,
    });
    bindings.push(Binding {
        source,
        dest: PathBuf::from(SNAPSHOT_BIND_DEST),
        mode,
    });
}

fn build_jailer_launch_config(
    input: JailerLaunchConfigInput<'_>,
    bindings: Vec<Binding>,
    sockets: Vec<JailerSocket>,
) -> JailerConfig {
    JailerConfig {
        jailer_bin: input.jailer_bin.to_path_buf(),
        jailer_harden_bin: Some(input.jailer_harden_bin.to_path_buf()),
        firecracker_bin: input.firecracker_bin.to_path_buf(),
        run_dir: input.run_dir.to_path_buf(),
        uid: input.uid,
        gid: input.gid,
        bindings,
        sockets,
        resource_limits: m80_jailer::ResourceLimits::default(),
        new_pid_ns: true,
        new_net_ns: input.private_netns,
        daemonize: input.daemonize,
        new_cgroup_ns: false,
        cgroup_version: Some(m80_jailer::CgroupVersion::V2),
        netns_path: input.netns_path.map(Path::to_path_buf),
        seccomp_filter_path: Some(PathBuf::from(FIRECRACKER_SECCOMP_FILTER_JAIL_PATH)),
        stdio_log: Some(console_log_path(input.run_dir)),
    }
}

fn cold_launch_netns_path(
    policy: &crate::NetworkPolicy,
    run_root: &Path,
    vm_id: &str,
) -> Option<PathBuf> {
    match policy {
        crate::NetworkPolicy::JoinNetns { spec } => Some(spec.netns_path.clone()),
        crate::NetworkPolicy::AllowOutbound { .. } => {
            Some(m80_net_outbound::planned_vmm_netns_path(run_root, vm_id))
        }
        crate::NetworkPolicy::NoEgress => None,
    }
}

fn join_netns_path(policy: &crate::NetworkPolicy) -> Option<&Path> {
    match policy {
        crate::NetworkPolicy::JoinNetns { spec } => Some(spec.netns_path.as_path()),
        crate::NetworkPolicy::NoEgress | crate::NetworkPolicy::AllowOutbound { .. } => None,
    }
}

fn private_vmm_netns(policy: &crate::NetworkPolicy) -> bool {
    matches!(policy, crate::NetworkPolicy::NoEgress)
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
            Err(e) => Err(FcError::Cgroup(e)),
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
    config: &SandboxConfig,
    jail: &m80_jailer::MaterializedJail,
    jailed: &m80_jailer::JailedFirecracker,
) -> Result<Option<Subtree>, FcError> {
    fail_cgroup_create_if_requested(vm_id)?;
    match mode {
        CgroupMode::Disabled => Ok(None),
        CgroupMode::UnifiedV2 => {
            // FcError::Cgroup wraps CgroupError via #[from]; `?` does the
            // conversion so we keep the structured cause for the CLI's
            // error → exit-code map.
            let mut limits = Limits::preset();
            limits.cpuset_cpus = config.cpuset_cpus.clone();
            let subtree = Subtree::create(vm_id, jail, jailed, &limits)?;
            Ok(Some(subtree))
        }
    }
}

#[cfg(debug_assertions)]
fn fail_cgroup_create_if_requested(vm_id: &str) -> Result<(), FcError> {
    if std::env::var_os(FAIL_CGROUP_CREATE_FOR_VM_ENV).as_deref()
        != Some(std::ffi::OsStr::new(vm_id))
    {
        return Ok(());
    }
    let path = PathBuf::from("/sys/fs/cgroup/m80-firecracker").join(vm_id);
    Err(FcError::Cgroup(m80_cgroup::CgroupError::Io {
        path,
        source: std::io::Error::from(std::io::ErrorKind::PermissionDenied),
    }))
}

#[cfg(not(debug_assertions))]
fn fail_cgroup_create_if_requested(_vm_id: &str) -> Result<(), FcError> {
    Ok(())
}

/// Phase 6: resolve the network mode and realize any m80-owned host links.
fn phase_6_network_realize(
    network_helper: &crate::network_helper::NetworkHelperClient,
    config: &SandboxConfig,
    vm_id: &str,
    run_root: &Path,
    run_dir: &Path,
) -> Result<RealizedNetwork, FcError> {
    match m80_net_mode::resolve(&config.network) {
        VmNetworkMode::NoEgress => Ok(RealizedNetwork::NoEgress),
        VmNetworkMode::JoinNetns { spec } => Ok(RealizedNetwork::JoinNetns {
            netns_path: spec.netns_path,
            tap_name: spec.tap_name,
            guest_mac: spec.guest_mac.to_string(),
            guest_ipv4: spec.guest_ipv4.to_string(),
            gateway_ipv4: spec.gateway_ipv4.to_string(),
            dns_resolvers: spec
                .dns_resolvers
                .iter()
                .map(std::string::ToString::to_string)
                .collect(),
        }),
        VmNetworkMode::OutboundNat { plan } => {
            let realized = network_helper.realize_bridge_and_tap(plan, vm_id, run_root, run_dir)?;
            Ok(RealizedNetwork::OutboundNat {
                tap_name: realized.tap_name,
                vmm_netns_path: realized.vmm_netns_path,
                guest_mac: realized.guest_mac,
            })
        }
    }
}

/// Phase 7: finalize OutboundNat guest boot tokens and host firewall policy.
fn phase_7_outbound_guest_config(
    network_helper: &crate::network_helper::NetworkHelperClient,
    network: &RealizedNetwork,
    run_dir: &Path,
) -> Result<Vec<String>, FcError> {
    match network {
        RealizedNetwork::OutboundNat { .. } => {
            let mut state = m80_net_outbound::read_vm_network_state_record(run_dir)?;
            let cmdline = m80_net_outbound::prepare_pid_one_network_cmdline(&mut state)?;
            network_helper.apply_outbound_nat_policy(run_dir)?;
            Ok(cmdline.args)
        }
        RealizedNetwork::JoinNetns {
            guest_mac,
            guest_ipv4,
            gateway_ipv4,
            dns_resolvers,
            ..
        } => Ok(vec![
            "m80.net=join_netns".to_owned(),
            "m80.net.iface=eth0".to_owned(),
            format!("m80.net.mac={guest_mac}"),
            format!("m80.net.ipv4={guest_ipv4}"),
            format!("m80.net.gateway={gateway_ipv4}"),
            format!("m80.net.dns={}", dns_resolvers.join(",")),
        ]),
        RealizedNetwork::NoEgress => Ok(Vec::new()),
    }
}

/// Phase 10: open the Firecracker UDS REST client.
///
/// Waits for the API socket creation event instead of sleeping on a fixed
/// interval. A short capped backoff is used only when inotify is unavailable
/// or a created socket is not accepting connections yet.
fn phase_10_open_uds(api_socket: &Path) -> Result<Client, FcError> {
    phase_10_open_uds_with_wait(api_socket, wait_for_api_socket_create)
}

fn phase_10_open_uds_with_wait(
    api_socket: &Path,
    mut wait_for_create: impl FnMut(&Path, Instant) -> Result<(), FcError>,
) -> Result<Client, FcError> {
    let deadline = Instant::now() + API_SOCKET_TIMEOUT;
    let mut fallback_delay = Duration::from_millis(1);
    loop {
        match Client::new(api_socket) {
            Ok(client) => return Ok(client),
            Err(e) if api_socket.exists() && Instant::now() >= deadline => {
                return Err(FcError::Client(e));
            }
            Err(e) if api_socket.exists() => {
                tracing::debug!(err = %e, "phase_10_open_uds: Client::new failed, retrying");
                sleep_api_socket_backoff(&mut fallback_delay, deadline);
            }
            Err(_) if Instant::now() >= deadline => {
                return Err(FcError::ApiSocketTimeout {
                    path: api_socket.to_path_buf(),
                    timeout: API_SOCKET_TIMEOUT,
                });
            }
            Err(e) => {
                tracing::debug!(err = %e, "phase_10_open_uds: API socket missing, waiting for create event");
                if let Err(err) = wait_for_create(api_socket, deadline) {
                    tracing::debug!(err = %err, "phase_10_open_uds: event wait failed, using short fallback backoff");
                    sleep_api_socket_backoff(&mut fallback_delay, deadline);
                }
            }
        }

        if Instant::now() >= deadline {
            return Err(FcError::ApiSocketTimeout {
                path: api_socket.to_path_buf(),
                timeout: API_SOCKET_TIMEOUT,
            });
        }
    }
}

fn sleep_api_socket_backoff(delay: &mut Duration, deadline: Instant) {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return;
    }
    std::thread::sleep(remaining.min(*delay));
    *delay = (*delay * 2).min(Duration::from_millis(25));
}

#[cfg(target_os = "linux")]
fn wait_for_api_socket_create(api_socket: &Path, deadline: Instant) -> Result<(), FcError> {
    use nix::sys::inotify::{AddWatchFlags, InitFlags, Inotify};

    let parent = api_socket.parent().ok_or_else(|| {
        path_io(
            api_socket,
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("api socket has no parent: {}", api_socket.display()),
            ),
        )
    })?;
    let filename = api_socket.file_name().ok_or_else(|| {
        path_io(
            api_socket,
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("api socket has no filename: {}", api_socket.display()),
            ),
        )
    })?;

    let inotify = Inotify::init(InitFlags::IN_NONBLOCK | InitFlags::IN_CLOEXEC)
        .map_err(|errno| errno_path_io(api_socket, errno))?;
    inotify
        .add_watch(
            parent,
            AddWatchFlags::IN_CREATE | AddWatchFlags::IN_MOVED_TO | AddWatchFlags::IN_ATTRIB,
        )
        .map_err(|errno| errno_path_io(api_socket, errno))?;

    if api_socket.exists() {
        return Ok(());
    }

    loop {
        wait_for_fd_readable(inotify.as_fd(), api_socket, deadline)?;
        match inotify.read_events() {
            Ok(events) => {
                if events
                    .iter()
                    .any(|event| event_name_matches(event.name.as_deref(), filename))
                    || api_socket.exists()
                {
                    return Ok(());
                }
            }
            Err(Errno::EAGAIN) | Err(Errno::EINTR) => continue,
            Err(errno) => return Err(errno_path_io(api_socket, errno)),
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn wait_for_api_socket_create(api_socket: &Path, _deadline: Instant) -> Result<(), FcError> {
    Err(path_io(
        api_socket,
        std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "inotify is only available on Linux",
        ),
    ))
}

fn event_name_matches(event_name: Option<&OsStr>, filename: &OsStr) -> bool {
    event_name.is_some_and(|name| name == filename)
}

fn wait_for_fd_readable(
    fd: std::os::unix::prelude::BorrowedFd<'_>,
    path: &Path,
    deadline: Instant,
) -> Result<(), FcError> {
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(FcError::ApiSocketTimeout {
                path: path.to_path_buf(),
                timeout: API_SOCKET_TIMEOUT,
            });
        }
        let mut fds = [PollFd::new(fd, PollFlags::POLLIN)];
        match poll(&mut fds, poll_timeout_for_duration(remaining, path)?) {
            Ok(0) => {
                return Err(FcError::ApiSocketTimeout {
                    path: path.to_path_buf(),
                    timeout: API_SOCKET_TIMEOUT,
                });
            }
            Ok(_) => return Ok(()),
            Err(Errno::EINTR) => continue,
            Err(errno) => return Err(errno_path_io(path, errno)),
        }
    }
}

fn poll_timeout_for_duration(duration: Duration, path: &Path) -> Result<PollTimeout, FcError> {
    PollTimeout::try_from(duration).map_err(|e| {
        path_io(
            path,
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("poll timeout out of range: {e}"),
            ),
        )
    })
}

fn errno_path_io(path: &Path, errno: Errno) -> FcError {
    path_io(path, std::io::Error::from_raw_os_error(errno as i32))
}

fn path_io(path: &Path, source: std::io::Error) -> FcError {
    FcError::PathIo {
        path: path.to_path_buf(),
        source,
    }
}

/// Phase 11: PUT all Firecracker resources in the documented order.
///
/// From Firecracker's perspective, resources must be PUT before `InstanceStart`:
/// machine-config → boot-source → drives (root first) → optional pmem layers
/// → optional network NIC → entropy → vsock.
#[allow(clippy::too_many_arguments)]
fn phase_11_rest_puts(
    client: &Client,
    storage: &StoragePrep,
    config: &SandboxConfig,
    vm_id: &str,
    image_kind: m80_image_manifest::ImageKind,
    kernel_kind: m80_image_manifest::KernelKind,
    rootfs_format: m80_image_manifest::RootfsFormat,
    network: &RealizedNetwork,
    extra_boot_args: &[String],
) -> Result<(), FcError> {
    let puts = plan_preboot_puts(
        config,
        vm_id,
        image_kind,
        kernel_kind,
        rootfs_format,
        storage.scratch.is_some(),
        &config.pmem_layers,
        &storage.pmem_backings,
        network,
        extra_boot_args,
    )?;
    apply_preboot_puts(client, &puts, vm_id)
}

fn record_post_launch_resource_snapshot(
    diagnostics: &mut Option<m80_observability::Diagnostics>,
    vm_id: &str,
    request_id: Option<&str>,
    firecracker_pid: u32,
    cgroup_enabled: bool,
) {
    let mut context = BTreeMap::new();
    context.insert("firecracker_pid".to_owned(), firecracker_pid.to_string());
    context.extend(proc_io_snapshot(firecracker_pid));
    if let Some(major_faults) = proc_stat_major_faults(firecracker_pid) {
        context.insert(
            "proc_stat_major_faults".to_owned(),
            major_faults.to_string(),
        );
    }
    if cgroup_enabled {
        context.extend(cgroup_cpu_stat_snapshot(vm_id));
    } else {
        context.insert("cgroup_cpu_stat_status".to_owned(), "disabled".to_owned());
    }
    crate::diagnostics::record_context(
        diagnostics,
        Phase::Ready,
        vm_id,
        request_id,
        "post_launch_resource_snapshot",
        context,
    );
}

fn proc_io_snapshot(pid: u32) -> BTreeMap<String, String> {
    let path = PathBuf::from("/proc").join(pid.to_string()).join("io");
    let Ok(text) = std::fs::read_to_string(&path) else {
        let mut context = BTreeMap::new();
        context.insert("proc_io_status".to_owned(), "unreadable".to_owned());
        context.insert("proc_io_path".to_owned(), path.display().to_string());
        return context;
    };
    parse_proc_io_text(&text)
}

fn parse_proc_io_text(text: &str) -> BTreeMap<String, String> {
    let mut context = BTreeMap::new();
    for line in text.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        if !key.is_empty() && !value.is_empty() {
            context.insert(format!("proc_io_{key}"), value.to_owned());
        }
    }
    context
}

fn proc_stat_major_faults(pid: u32) -> Option<u64> {
    let path = PathBuf::from("/proc").join(pid.to_string()).join("stat");
    let text = std::fs::read_to_string(path).ok()?;
    proc_stat_major_faults_from_text(&text)
}

fn proc_stat_major_faults_from_text(text: &str) -> Option<u64> {
    let after_comm = text.rsplit_once(") ")?.1;
    after_comm.split_whitespace().nth(9)?.parse().ok()
}

fn cgroup_cpu_stat_snapshot(vm_id: &str) -> BTreeMap<String, String> {
    let path = m80_cgroup::Subtree::leaf_path(vm_id).join("cpu.stat");
    let Ok(text) = std::fs::read_to_string(&path) else {
        let mut context = BTreeMap::new();
        context.insert("cgroup_cpu_stat_status".to_owned(), "unreadable".to_owned());
        context.insert(
            "cgroup_cpu_stat_path".to_owned(),
            path.display().to_string(),
        );
        return context;
    };
    let mut context = BTreeMap::new();
    context.insert("cgroup_cpu_stat_status".to_owned(), "ok".to_owned());
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        let Some(key) = parts.next() else {
            continue;
        };
        let Some(value) = parts.next() else {
            continue;
        };
        context.insert(format!("cgroup_cpu_stat_{key}"), value.to_owned());
    }
    context
}

#[cfg(test)]
mod tests;
