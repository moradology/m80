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
use std::time::{Duration, Instant};

use m80_cgroup::{Limits, Subtree};
use m80_firecracker_client::{
    BootSourceConfig, Client, DriveConfig, InstanceAction, MachineConfig, VsockConfig,
};
use m80_image_manifest::Manifest;
use m80_jailer::{BindMode, Binding, JailerConfig, Plan, SocketSpec};
use m80_net_mode::VmNetworkMode;
use m80_preflight::Discovery;
use m80_proto::READY_PORT_DEFAULT;
use m80_snapshot::{restore as snapshot_restore, RestoreRequest, SnapshotPaths};
use m80_storage::{Rootfs, Scratch};
use m80_vsock::{Channel, GUEST_PORT_DEFAULT};

use crate::error::FcError;
use crate::runroot::write_ownership_lock;
use crate::timing::phase;
use crate::types::{
    CgroupMode, RealizedNetwork, RunningSandbox, Sandbox, SandboxConfig, StoragePrep,
};

/// Default scratch size: 64 MiB.
const SCRATCH_DEFAULT_BYTES: u64 = 64 * 1024 * 1024;

/// Default vCPU count.
const DEFAULT_VCPU_COUNT: u32 = 1;

/// Default memory in MiB.
const DEFAULT_MEM_SIZE_MIB: u32 = 1024;

/// Common kernel command-line arguments for Stock kernels.
///
/// `panic=-1` triggers immediate reboot on kernel panic (vs. `panic=1`'s
/// 1 s wait). For minimal-kind images where m80-guestd is PID 1, the
/// graceful-stop path exits PID 1 → kernel panics → Firecracker exits;
/// the 1 s wait was pure dead time on every launch.
const COMMON_BOOT_ARGS: &str = "console=ttyS0 reboot=k panic=-1 pci=off";

/// Kernel command-line arguments for Stripped kernels.
///
/// Differences from `COMMON_BOOT_ARGS`:
/// - `pci=off` removed — `CONFIG_PCI=n` in the stripped kernel makes this
///   flag a no-op; removing it keeps the cmdline honest.
/// - `quiet loglevel=0` added — suppresses per-device init messages on ttyS0
///   while leaving the console open; fatal panics still print (the panic
///   handler bypasses loglevel). Saves ~20-40 ms of serial flush time on boot.
/// - `8250.nr_uarts=1` added — explicit single-UART cap; prevents probe of
///   the four default UARTs on driver init. Locked at `=1` (not `=0`) per
///   CLAUDE.md "diagnostics before hypotheses": preserving console output is
///   worth more than the ~50 ms saving from suppressing it entirely.
const STRIPPED_BOOT_ARGS: &str =
    "console=ttyS0 reboot=k panic=-1 quiet loglevel=0 8250.nr_uarts=1";

/// Build kernel boot args for the given `(image_kind, kernel_kind)` pair,
/// honoring any caller override on `SandboxConfig::boot_args`.
///
/// Matrix:
/// - `(Ubuntu, Stock)`: `COMMON_BOOT_ARGS` — systemd is the kernel's `init=`
///   (kernel defaults to `/sbin/init`).
/// - `(Ubuntu, Stripped)`: `STRIPPED_BOOT_ARGS` — systemd is still the init;
///   no `init=` override needed.
/// - `(Minimal, Stock)`: `COMMON_BOOT_ARGS init=/m80-guestd` — explicit `init=`
///   so the kernel calls our PID-1-aware daemon directly. The minimal rootfs
///   also has `/init -> /m80-guestd` as a backstop.
/// - `(Minimal, Stripped)`: `STRIPPED_BOOT_ARGS init=/m80-guestd` — same
///   belt-and-suspenders `init=` retained; kernel symlink is the backstop.
fn boot_args_for(
    kind: m80_image_manifest::ImageKind,
    kernel_kind: m80_image_manifest::KernelKind,
    config_override: Option<&str>,
) -> String {
    if let Some(custom) = config_override {
        return custom.to_owned();
    }
    use m80_image_manifest::{ImageKind, KernelKind};
    match (kind, kernel_kind) {
        (ImageKind::Ubuntu, KernelKind::Stock) => COMMON_BOOT_ARGS.to_owned(),
        (ImageKind::Ubuntu, KernelKind::Stripped) => STRIPPED_BOOT_ARGS.to_owned(),
        (ImageKind::Minimal, KernelKind::Stock) => {
            format!("{COMMON_BOOT_ARGS} init=/m80-guestd")
        }
        (ImageKind::Minimal, KernelKind::Stripped) => {
            format!("{STRIPPED_BOOT_ARGS} init=/m80-guestd")
        }
    }
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

impl Sandbox {
    /// Standalone constructor for callers without a `Backend`.
    ///
    /// Deferred to v0.2. The admission semaphore is bypassed in this path,
    /// but constructing a minimal Backend without a Discovery from preflight
    /// is not supported in v0.1. Use `Backend::admit` instead.
    pub fn new(_config: SandboxConfig) -> Result<Sandbox, FcError> {
        Err(FcError::Config(
            "Sandbox::new is not supported in v0.1; use Backend::admit(config).launch()".into(),
        ))
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

        // Phase 2: lease acquisition. NB: name the binding `_lease_guard`
        // (suffix after underscore) — a bare `_lease` would drop the guard
        // immediately at the end of the let-statement, removing the
        // ownership.lock before phase 3 runs.
        let _lease_guard = phase("phase_2_lease", &vm_id, || write_ownership_lock(&run_dir))?;

        // Phase 3: manifest verify + storage prep.
        let storage = phase("phase_3_storage_prep", &vm_id, || {
            phase_3_storage_prep(
                &backend_config.discovery.manifest,
                &backend_config.discovery.rootfs,
                &self.config,
                &run_dir,
            )
        })?;

        // Phase 4: jailer materialize.
        let jail = phase("phase_4_jailer_materialize", &vm_id, || {
            phase_4_jailer_materialize(
                &backend_config.discovery.jailer_bin,
                &backend_config.discovery.firecracker_bin,
                backend_config.jail_uid,
                backend_config.jail_gid,
                &run_dir,
                &backend_config.discovery.kernel,
                &storage,
            )
        })?;

        // Phase 5: probe cgroup availability (creation happens after phase 9).
        phase("phase_5_cgroup_probe", &vm_id, || {
            phase_5_cgroup_probe(backend_config.cgroup_mode)
        })?;

        // Phase 6: resolve network mode.
        let net = phase("phase_6_network_realize", &vm_id, || {
            phase_6_network_realize(&self.config)
        })?;

        // Phase 7: guest config injection — no-op in v0.1 (only OutboundNat
        // needs in-VM config and that mode is deferred). The `net` value is
        // still consumed by phase_11 below for the NIC PUT.

        // Phase 8: compute the API socket path (inside the jail root).
        let api_socket = jail.jail_path.join("firecracker.sock");

        // Phase 9: jailer exec's firecracker. Returns live pids.
        let firecracker = phase("phase_9_jailer_launch", &vm_id, || {
            jail.launch(&api_socket).map_err(FcError::Jailer)
        })?;

        // Phase 5b: create cgroup subtree now that we have live pids.
        let cgroup = phase("phase_5b_cgroup_create", &vm_id, || {
            phase_5b_cgroup_create(backend_config.cgroup_mode, &vm_id, &jail, &firecracker)
        })?;

        // Phase 10: open UDS REST client (retries for up to 5 s).
        let host_api_socket = jail.jail_path.join("firecracker.sock");
        let client = phase("phase_10_open_uds", &vm_id, || {
            phase_10_open_uds(&host_api_socket)
        })?;

        // Phase 11: REST PUTs in documented order.
        phase("phase_11_rest_puts", &vm_id, || {
            phase_11_rest_puts(
                &client,
                &storage,
                &net,
                &self.config,
                &backend_config.discovery.kernel,
                &run_dir,
                &vm_id,
                backend_config.discovery.manifest.image_kind,
                backend_config.discovery.manifest.kernel_kind,
            )
        })?;

        // Phase 11b: pre-create the inverted-readiness UnixListener at
        // `<jail>/vsock.sock_<READY_PORT_DEFAULT>`. Firecracker's muxer
        // connects to this path when the guest does outbound to the
        // ready port; if it doesn't exist when that happens, the muxer
        // RSTs the guest. Must be created before InstanceStart.
        let vsock_uds = jail.jail_path.join("vsock.sock");
        let ready_uds = jail
            .jail_path
            .join(format!("vsock.sock_{READY_PORT_DEFAULT}"));
        let ready_listener = phase("phase_11b_ready_listener_bind", &vm_id, || {
            phase_11b_bind_ready_listener(&ready_uds, backend_config.jail_uid)
        })?;

        // Phase 12a: InstanceStart.
        phase("phase_12a_instance_start", &vm_id, || {
            client.instance_action(InstanceAction::InstanceStart)
        })?;

        // Phase 12b: accept the inverted-readiness signal from m80-guestd,
        // then open the exec channel. accept() returns event-driven the
        // moment guestd's outbound connect lands — no muxer-polling race.
        let channel = phase("phase_12b_ready_accept", &vm_id, || {
            phase_12b_ready_accept(&ready_listener, &vsock_uds, &vm_id)
        })?;

        Ok(RunningSandbox {
            vm_id,
            run_dir,
            jail,
            cgroup,
            channel,
            rootfs: storage.rootfs,
            scratch: storage.scratch,
            client,
            firecracker,
            permit: self.permit,
            backend: self.backend,
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

        // Phase 2: lease acquisition.
        let _lease_guard = phase("phase_2_lease", &vm_id, || write_ownership_lock(&run_dir))?;

        // Phase 3: storage prep (overlay + optional scratch — still needed
        // for the jailer bind-mount layout even on restore path).
        let storage = phase("phase_3_storage_prep", &vm_id, || {
            phase_3_storage_prep(
                &backend_config.discovery.manifest,
                &backend_config.discovery.rootfs,
                &self.config,
                &run_dir,
            )
        })?;

        // Phase 4: jailer materialize.
        let jail = phase("phase_4_jailer_materialize", &vm_id, || {
            phase_4_jailer_materialize(
                &backend_config.discovery.jailer_bin,
                &backend_config.discovery.firecracker_bin,
                backend_config.jail_uid,
                backend_config.jail_gid,
                &run_dir,
                &backend_config.discovery.kernel,
                &storage,
            )
        })?;

        // Phase 5: cgroup probe (restore path honours cgroup mode too).
        phase("phase_5_cgroup_probe", &vm_id, || {
            phase_5_cgroup_probe(backend_config.cgroup_mode)
        })?;

        // Phase 8: compute the API socket path (inside the jail root).
        let api_socket = jail.jail_path.join("firecracker.sock");

        // Phase 9: spawn Firecracker via jailer.
        let firecracker = phase("phase_9_jailer_launch", &vm_id, || {
            jail.launch(&api_socket).map_err(FcError::Jailer)
        })?;

        // Phase 5b: create cgroup subtree.
        let cgroup = phase("phase_5b_cgroup_create", &vm_id, || {
            phase_5b_cgroup_create(backend_config.cgroup_mode, &vm_id, &jail, &firecracker)
        })?;

        // Phase 10: open UDS REST client.
        let host_api_socket = jail.jail_path.join("firecracker.sock");
        let client = phase("phase_10_open_uds", &vm_id, || {
            phase_10_open_uds(&host_api_socket)
        })?;

        // Phase restore-load: remove stale vsock.sock + PUT /snapshot/load +
        // PATCH /vm Resumed (resume: true).
        let vsock_uds = jail.jail_path.join("vsock.sock");
        phase("phase_restore_load", &vm_id, || {
            snapshot_restore(RestoreRequest {
                fc_socket: host_api_socket.clone(),
                paths: snapshot,
                vsock_uds: vsock_uds.clone(),
                resume: true,
            })
            .map_err(FcError::Snapshot)
        })?;

        // Phase restore-probe: CONNECT 9001 retry loop.
        // Replaces the cold-boot phase_12b_ready_accept.
        let channel = phase("phase_restore_probe_exec_channel", &vm_id, || {
            phase_restore_probe_exec_channel(&vsock_uds, &vm_id)
        })?;

        let _ = discovery; // Discovery is passed for API symmetry; not needed beyond the phases above.

        Ok(RunningSandbox {
            vm_id,
            run_dir,
            jail,
            cgroup,
            channel,
            rootfs: storage.rootfs,
            scratch: storage.scratch,
            client,
            firecracker,
            permit: self.permit,
            backend: self.backend,
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
                    return Err(FcError::Vsock(m80_vsock::VsockError::NotReady));
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
    let run_dir = run_root.join(vm_id);
    std::fs::create_dir_all(&run_dir)?;
    Ok(run_dir)
}

/// Phase 3: verify the manifest sha256s and prepare the rootfs overlay;
/// optionally create a scratch image for the workspace.
fn phase_3_storage_prep(
    manifest: &Manifest,
    base_rootfs: &Path,
    config: &SandboxConfig,
    run_dir: &Path,
) -> Result<StoragePrep, FcError> {
    // Verify all six artifact sha256s against on-disk files. Preflight already
    // verified at startup; we re-verify here to guard against TOCTOU drift.
    let manifest_dir = base_rootfs.parent().unwrap_or(std::path::Path::new("/"));
    manifest.verify(manifest_dir)?;

    // Allocate a sparse per-VM overlay ext4; the base is NOT copied.
    let overlay_dest = run_dir.join("rootfs.overlay.ext4");
    let rootfs = Rootfs::prepare(base_rootfs, &overlay_dest, config.overlay_size_bytes)?;

    let scratch = if let Some(workspace) = &config.workspace {
        let scratch_dest = run_dir.join("scratch.ext4");
        let scratch = Scratch::create(workspace, &scratch_dest, SCRATCH_DEFAULT_BYTES)?;
        Some(scratch)
    } else {
        None
    };

    Ok(StoragePrep { rootfs, scratch })
}

/// Phase 4: compute a `JailerConfig`, run `Plan::compute`, and materialize.
fn phase_4_jailer_materialize(
    jailer_bin: &Path,
    firecracker_bin: &Path,
    uid: u32,
    gid: u32,
    run_dir: &Path,
    kernel: &Path,
    storage: &StoragePrep,
) -> Result<m80_jailer::MaterializedJail, FcError> {
    let mut bindings = vec![
        // Kernel — read-only inside the jail.
        Binding {
            source: kernel.to_path_buf(),
            dest: PathBuf::from("kernel"),
            mode: BindMode::Ro,
        },
        // Shared read-only base ext4 (vda). Same host file across all VMs;
        // bind RO so the jail cannot mutate the shared image.
        Binding {
            source: storage.rootfs.base_path().to_path_buf(),
            dest: PathBuf::from("rootfs.ext4"),
            mode: BindMode::Ro,
        },
        // Per-VM sparse overlay ext4 (vdb). Writable; holds all guest writes.
        Binding {
            source: storage.rootfs.overlay_path().to_path_buf(),
            dest: PathBuf::from("rootfs.overlay.ext4"),
            mode: BindMode::Rw,
        },
    ];

    if let Some(scratch) = &storage.scratch {
        bindings.push(Binding {
            source: scratch.path().to_path_buf(),
            dest: PathBuf::from("scratch.ext4"),
            mode: BindMode::Rw,
        });
    }

    let sockets = vec![
        SocketSpec {
            path: PathBuf::from("firecracker.sock"),
        },
        SocketSpec {
            path: PathBuf::from("vsock.sock"),
        },
    ];

    let jailer_config = JailerConfig {
        jailer_bin: jailer_bin.to_path_buf(),
        firecracker_bin: firecracker_bin.to_path_buf(),
        run_dir: run_dir.to_path_buf(),
        uid,
        gid,
        bindings,
        sockets,
    };

    let plan = Plan::compute(&jailer_config)?;
    plan.materialize().map_err(FcError::Jailer)
}

/// Phase 5: probe that cgroup v2 is available when `UnifiedV2` mode is
/// requested. Actual subtree creation happens in phase 5b (after launch).
fn phase_5_cgroup_probe(mode: CgroupMode) -> Result<(), FcError> {
    match mode {
        CgroupMode::Disabled => Ok(()),
        CgroupMode::UnifiedV2 => match Subtree::probe() {
            Ok(()) => Ok(()),
            Err(m80_cgroup::CgroupError::UnsupportedHostMode) => Err(FcError::Config(
                "cgroup mode UnifiedV2 requested but host is not v2".into(),
            )),
            Err(e) => Err(FcError::Config(format!("cgroup probe: {e}"))),
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
            let subtree = Subtree::create(vm_id, jail, jailed)?;
            subtree.apply_limits(&Limits::default())?;
            Ok(Some(subtree))
        }
    }
}

/// Phase 6: resolve the network mode. Rejects `OutboundNat` (deferred to v0.2).
fn phase_6_network_realize(config: &SandboxConfig) -> Result<RealizedNetwork, FcError> {
    match m80_net_mode::resolve(&config.network) {
        VmNetworkMode::NoEgress => Ok(RealizedNetwork::NoEgress),
        VmNetworkMode::OutboundNat { .. } => Err(FcError::Config(
            "OutboundNat networking is deferred to v0.2; use NetworkPolicy::NoEgress in v0.1"
                .into(),
        )),
    }
}

/// Phase 10: open the Firecracker UDS REST client.
///
/// Retries for up to 5 s to allow Firecracker to create the socket after
/// jailer exec.
fn phase_10_open_uds(api_socket: &Path) -> Result<Client, FcError> {
    let deadline = Instant::now() + Duration::from_secs(5);
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
            return Err(FcError::Config(format!(
                "Firecracker API socket {} did not appear within 5 s",
                api_socket.display()
            )));
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
    _net: &RealizedNetwork,
    config: &SandboxConfig,
    _kernel: &Path,
    _run_dir: &Path,
    vm_id: &str,
    image_kind: m80_image_manifest::ImageKind,
    kernel_kind: m80_image_manifest::KernelKind,
) -> Result<(), FcError> {
    // a. Machine config.
    client.put_machine_config(&MachineConfig {
        vcpu_count: config.vcpu_count.unwrap_or(DEFAULT_VCPU_COUNT),
        mem_size_mib: config.mem_size_mib.unwrap_or(DEFAULT_MEM_SIZE_MIB),
        smt: false,
    })?;

    // b. Boot source. The kernel is bind-mounted at `/kernel` inside the
    // jailer chroot; Firecracker sees that path from within its chroot.
    let boot_args = boot_args_for(image_kind, kernel_kind, config.boot_args.as_deref());
    client.put_boot_source(&BootSourceConfig {
        kernel_image_path: PathBuf::from("/kernel"),
        boot_args: Some(boot_args),
        initrd_path: None,
    })?;

    // c. vda: shared read-only base ext4. is_root_device=true; is_read_only
    // must be set explicitly (Firecracker REST default is false per design doc).
    client.put_drive(&DriveConfig {
        drive_id: "rootfs".into(),
        path_on_host: PathBuf::from("/rootfs.ext4"),
        is_root_device: true,
        is_read_only: true,
    })?;

    // d. vdb: per-VM sparse overlay ext4. Writable; guestd mounts this as
    // the overlayfs upperdir after pivot_root.
    client.put_drive(&DriveConfig {
        drive_id: "rootfs_overlay".into(),
        path_on_host: PathBuf::from("/rootfs.overlay.ext4"),
        is_root_device: false,
        is_read_only: false,
    })?;

    // e. vdc: workspace drive (optional). Only when workspace is configured.
    if storage.scratch.is_some() {
        client.put_drive(&DriveConfig {
            drive_id: "workspace".into(),
            path_on_host: PathBuf::from("/scratch.ext4"),
            is_root_device: false,
            is_read_only: false,
        })?;
    }

    // e. Vsock device.
    let guest_cid = m80_vsock::cid_for_vm_id(vm_id);
    // The vsock UDS is at `/vsock.sock` inside the chroot.
    client.put_vsock(&VsockConfig {
        guest_cid,
        uds_path: PathBuf::from("/vsock.sock"),
    })?;

    // f. NIC PUT only for OutboundNat — not applicable in v0.1.

    Ok(())
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
    chown(path, Some(Uid::from_raw(jail_uid)), None).map_err(|e| {
        FcError::Io(std::io::Error::from_raw_os_error(e as i32))
    })?;

    Ok(listener)
}

/// `accept()` the inverted-readiness signal from m80-guestd, validate the
/// protocol-version byte, then open the exec channel.
///
/// The host-local `accept()` poll-loop here is not the muxer-polling race
/// the original `phase_12b_ready_probe` had. We're polling our own
/// UnixListener; the muxer only fires once (when guestd does its outbound
/// connect). No EAGAIN cascade.
fn phase_12b_ready_accept(
    ready_listener: &UnixListener,
    vsock_uds: &Path,
    vm_id: &str,
) -> Result<Channel, FcError> {
    ready_listener.set_nonblocking(true).map_err(FcError::Io)?;
    let deadline = Instant::now() + READY_TIMEOUT;

    let mut stream = loop {
        match ready_listener.accept() {
            Ok((s, _)) => break s,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    return Err(FcError::Vsock(m80_vsock::VsockError::NotReady));
                }
                std::thread::sleep(READY_ACCEPT_POLL);
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
        return Err(FcError::Vsock(m80_vsock::VsockError::HandshakeFailed));
    }
    drop(stream);

    tracing::info!(vm_id, "ready signal received from guestd");

    // Open the exec channel — same UDS, exec port. Synchronous; should
    // succeed immediately since guestd is up.
    Channel::open_uds_only(vsock_uds, GUEST_PORT_DEFAULT).map_err(FcError::Vsock)
}

#[cfg(test)]
mod tests {
    use super::*;
    use m80_image_manifest::{ImageKind, KernelKind};

    #[test]
    fn boot_args_ubuntu_stock() {
        assert_eq!(
            boot_args_for(ImageKind::Ubuntu, KernelKind::Stock, None),
            "console=ttyS0 reboot=k panic=-1 pci=off",
        );
    }

    #[test]
    fn boot_args_ubuntu_stripped() {
        assert_eq!(
            boot_args_for(ImageKind::Ubuntu, KernelKind::Stripped, None),
            "console=ttyS0 reboot=k panic=-1 quiet loglevel=0 8250.nr_uarts=1",
        );
    }

    #[test]
    fn boot_args_minimal_stock() {
        assert_eq!(
            boot_args_for(ImageKind::Minimal, KernelKind::Stock, None),
            "console=ttyS0 reboot=k panic=-1 pci=off init=/m80-guestd",
        );
    }

    #[test]
    fn boot_args_minimal_stripped() {
        assert_eq!(
            boot_args_for(ImageKind::Minimal, KernelKind::Stripped, None),
            "console=ttyS0 reboot=k panic=-1 quiet loglevel=0 8250.nr_uarts=1 init=/m80-guestd",
        );
    }

    #[test]
    fn boot_args_override_wins_over_kind_default() {
        let custom = "console=ttyS0 my=custom args";
        assert_eq!(
            boot_args_for(ImageKind::Minimal, KernelKind::Stripped, Some(custom)),
            custom,
            "explicit override must take precedence regardless of kind and kernel_kind"
        );
        assert_eq!(
            boot_args_for(ImageKind::Ubuntu, KernelKind::Stock, Some(custom)),
            custom,
        );
    }
}
