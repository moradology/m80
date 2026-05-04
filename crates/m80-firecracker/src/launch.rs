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
//! - **Phase 12b (ready probe)**: instead of watching the Firecracker serial
//!   console for `GUESTD_READY` via [`Channel::open`], v0.1 polls the host
//!   vsock UDS by attempting `Channel::open` every 10 ms with a 60 s
//!   deadline and a 1-second per-attempt timeout. A `PUT /logger` REST call
//!   would be needed to make Firecracker write the serial console to a
//!   host-visible file; that is deferred to v0.2.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use m80_cgroup::{Limits, Subtree};
use m80_firecracker_client::{
    BootSourceConfig, Client, DriveConfig, InstanceAction, MachineConfig, VsockConfig,
};
use m80_image_manifest::Manifest;
use m80_jailer::{BindMode, Binding, JailerConfig, Plan, SocketSpec};
use m80_net_mode::VmNetworkMode;
use m80_storage::{Rootfs, Scratch};
use m80_vsock::{Channel, GUEST_PORT_DEFAULT};

use crate::error::FcError;
use crate::runroot::write_ownership_lock;
use crate::types::{
    CgroupMode, RealizedNetwork, RunningSandbox, Sandbox, SandboxConfig, StoragePrep,
};

/// Default scratch size: 64 MiB.
const SCRATCH_DEFAULT_BYTES: u64 = 64 * 1024 * 1024;

/// Default vCPU count.
const DEFAULT_VCPU_COUNT: u32 = 1;

/// Default memory in MiB.
const DEFAULT_MEM_SIZE_MIB: u32 = 1024;

/// Default boot args.
const DEFAULT_BOOT_ARGS: &str = "console=ttyS0 reboot=k panic=1 pci=off";

/// Ready probe: poll interval.
const READY_POLL_INTERVAL: Duration = Duration::from_millis(10);

/// Ready probe: total timeout.
const READY_TIMEOUT: Duration = Duration::from_secs(60);

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
        let run_dir = phase_1_run_root_prep(run_root, &vm_id)?;

        // Phase 2: lease acquisition. NB: name the binding `_lease_guard`
        // (suffix after underscore) — a bare `_lease` would drop the guard
        // immediately at the end of the let-statement, removing the
        // ownership.lock before phase 3 runs.
        let _lease_guard = write_ownership_lock(&run_dir)?;

        // Phase 3: manifest verify + storage prep.
        let storage = phase_3_storage_prep(
            &backend_config.discovery.manifest,
            &backend_config.discovery.rootfs,
            &self.config,
            &run_dir,
        )?;

        // Phase 4: jailer materialize.
        let jail = phase_4_jailer_materialize(
            &backend_config.discovery.jailer_bin,
            &backend_config.discovery.firecracker_bin,
            backend_config.jail_uid,
            backend_config.jail_gid,
            &run_dir,
            &backend_config.discovery.kernel,
            &storage,
        )?;

        // Phase 5: probe cgroup availability (creation happens after phase 9).
        phase_5_cgroup_probe(backend_config.cgroup_mode)?;

        // Phase 6: resolve network mode.
        let net = phase_6_network_realize(&self.config)?;

        // Phase 7: guest config injection — no-op in v0.1 (only OutboundNat
        // needs in-VM config and that mode is deferred). The `net` value is
        // still consumed by phase_11 below for the NIC PUT.

        // Phase 8: compute the API socket path (inside the jail root).
        let api_socket = jail.jail_path.join("firecracker.sock");

        // Phase 9: jailer exec's firecracker. Returns live pids.
        let firecracker = jail.launch(&api_socket).map_err(FcError::Jailer)?;

        // Phase 5b: create cgroup subtree now that we have live pids.
        let cgroup =
            phase_5b_cgroup_create(backend_config.cgroup_mode, &vm_id, &jail, &firecracker)?;

        // Phase 10: open UDS REST client (retries for up to 5 s).
        let host_api_socket = jail.jail_path.join("firecracker.sock");
        let client = phase_10_open_uds(&host_api_socket)?;

        // Phase 11: REST PUTs in documented order.
        phase_11_rest_puts(
            &client,
            &storage,
            &net,
            &self.config,
            &backend_config.discovery.kernel,
            &run_dir,
            &vm_id,
        )?;

        // Phase 12a: InstanceStart.
        client.instance_action(InstanceAction::InstanceStart)?;

        // Phase 12b: poll vsock UDS until guestd is ready. The jailer
        // materializes the socket inside the chroot, so the host-visible
        // path is `<jail_path>/vsock.sock`, not `<run_dir>/vsock.sock`.
        let vsock_uds = jail.jail_path.join("vsock.sock");
        let channel = phase_12b_ready_probe(&vsock_uds, &vm_id)?;

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

/// Phase 3: verify the manifest sha256s and clone the rootfs; optionally
/// create a scratch image for the workspace.
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

    let rootfs_dest = run_dir.join("rootfs.ext4");
    let rootfs = Rootfs::clone(base_rootfs, &rootfs_dest)?;

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
        // Per-VM rootfs clone — read-write (the guest writes into its own copy).
        Binding {
            source: storage.rootfs.path().to_path_buf(),
            dest: PathBuf::from("rootfs.ext4"),
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
fn phase_11_rest_puts(
    client: &Client,
    storage: &StoragePrep,
    _net: &RealizedNetwork,
    config: &SandboxConfig,
    _kernel: &Path,
    _run_dir: &Path,
    vm_id: &str,
) -> Result<(), FcError> {
    // a. Machine config.
    client.put_machine_config(&MachineConfig {
        vcpu_count: config.vcpu_count.unwrap_or(DEFAULT_VCPU_COUNT),
        mem_size_mib: config.mem_size_mib.unwrap_or(DEFAULT_MEM_SIZE_MIB),
        smt: false,
    })?;

    // b. Boot source. The kernel is bind-mounted at `/kernel` inside the
    // jailer chroot; Firecracker sees that path from within its chroot.
    let boot_args = config
        .boot_args
        .as_deref()
        .unwrap_or(DEFAULT_BOOT_ARGS)
        .to_owned();
    client.put_boot_source(&BootSourceConfig {
        kernel_image_path: PathBuf::from("/kernel"),
        boot_args: Some(boot_args),
        initrd_path: None,
    })?;

    // c. Root drive. The rootfs clone is bind-mounted at `/rootfs.ext4`.
    client.put_drive(&DriveConfig {
        drive_id: "rootfs".into(),
        path_on_host: PathBuf::from("/rootfs.ext4"),
        is_root_device: true,
        is_read_only: false,
    })?;

    // d. Workspace drive (optional).
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
/// [`Channel::open`] normally watches the Firecracker serial-console file for
/// the `GUESTD_READY` marker before connecting. Firecracker only writes to a
/// file-backed console when configured via `PUT /logger`; wiring that logger
/// is deferred to v0.2.
///
/// Instead, v0.1 calls `Channel::open` with a 1-second per-attempt timeout
/// (so the console watch times out quickly) and retries every 10 ms until
/// the vsock handshake succeeds or the 60-second overall deadline elapses.
fn phase_12b_ready_probe(vsock_uds: &Path, vm_id: &str) -> Result<Channel, FcError> {
    let deadline = Instant::now() + READY_TIMEOUT;

    loop {
        if vsock_uds.exists() {
            match Channel::open_uds_only(vsock_uds, GUEST_PORT_DEFAULT) {
                Ok(channel) => {
                    tracing::info!(vm_id, "vsock channel open: guestd ready");
                    return Ok(channel);
                }
                Err(m80_vsock::VsockError::ConnectFailed { errno }) => {
                    tracing::debug!(vm_id, errno, "vsock connect failed, retrying");
                }
                Err(m80_vsock::VsockError::HandshakeFailed) => {
                    tracing::debug!(vm_id, "vsock handshake failed, retrying");
                }
                Err(e) => return Err(FcError::Vsock(e)),
            }
        }
        if Instant::now() >= deadline {
            return Err(FcError::Vsock(m80_vsock::VsockError::NotReady));
        }
        std::thread::sleep(READY_POLL_INTERVAL);
    }
}
