//! Public struct and enum definitions for `m80-firecracker`.
//! Implementation blocks live in the module that owns the type's domain.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use m80_firecracker_client::{CacheType, Client, CpuTemplate, LogLevel as FcLogLevel};
use m80_jailer::{JailedFirecracker, MaterializedJail};
use m80_net_mode::NetworkPolicy;
use m80_storage::{OverlayTemplateCloneMode, Rootfs, Scratch};

use crate::network_helper::NetworkHelperClient;
use crate::pmem::PmemLayer;
use crate::runroot::LeaseGuard;

/// First-line Firecracker shape used by default and by snapshot timing proofs.
pub const FIRST_LINE_VCPU_COUNT: u32 = 1;

/// First-line Firecracker memory size, in MiB, used by default and by snapshot
/// timing proofs.
pub const FIRST_LINE_MEM_SIZE_MIB: u32 = 512;

/// Default count of preallocated hotplug drive slots.
pub(crate) const DEFAULT_PREALLOCATED_DRIVE_SLOTS: u8 = 0;

/// Shared admission semaphore (available permit count).
pub(crate) type Semaphore = Arc<Mutex<u32>>;

/// An acquired admission permit. Releasing it (via Drop) returns one slot to
/// the semaphore. Held inside `Sandbox` / `RunningSandbox` / `StoppedSandbox`
/// — callers don't construct these directly.
pub(crate) struct AdmissionPermit {
    /// Handle back to the semaphore so Drop can return the slot.
    pub(crate) sem: Semaphore,
    /// The configured maximum (for error messages on exhaustion).
    pub(crate) limit: u32,
}

impl std::fmt::Debug for AdmissionPermit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AdmissionPermit")
            .field("limit", &self.limit)
            .finish()
    }
}

impl Drop for AdmissionPermit {
    fn drop(&mut self) {
        let mut available = self.sem.lock().unwrap_or_else(|p| p.into_inner());
        *available += 1;
    }
}

/// Long-lived backend handle a service holds. Wraps the admission semaphore
/// and the discovery info from preflight. One per process.
///
/// Field-level access is `pub(crate)` — public callers go through
/// methods on `impl Backend`. (m80-cli reaches for `run_root` and the
/// effective config; both have explicit accessors.)
pub struct Backend {
    /// Merged + annotated configuration.
    pub(crate) config: BackendConfig,
    /// Effective configuration snapshot (for `show_effective_config`).
    pub(crate) effective: EffectiveConfig,
    /// Admission semaphore (available permit count).
    pub(crate) semaphore: Semaphore,
    /// Privileged outbound-network helper client.
    pub(crate) network_helper: Arc<NetworkHelperClient>,
}

impl Backend {
    /// The merged backend configuration (preflight discovery + admission
    /// limits + jail uid/gid + cgroup mode + run_root).
    #[must_use]
    pub fn config(&self) -> &BackendConfig {
        &self.config
    }
}

impl std::fmt::Debug for Backend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Backend")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

/// Configuration for [`Backend`]. Built by merging defaults, file config,
/// env vars, and CLI flags in that precedence order.
#[derive(Debug, Clone)]
pub struct BackendConfig {
    /// Discovery output from `m80-preflight::run()`.
    pub(crate) discovery: m80_preflight::Discovery,
    /// Maximum concurrent VMs admitted on this host.
    pub(crate) max_concurrent_vms: u32,
    /// Per-process run-root directory.
    pub(crate) run_root: PathBuf,
    /// UID for the jailed firecracker process.
    pub(crate) jail_uid: u32,
    /// GID for the jailed firecracker process.
    pub(crate) jail_gid: u32,
    /// Cgroup mode (unified-v2 or disabled).
    pub(crate) cgroup_mode: CgroupMode,
}

impl BackendConfig {
    /// Start building backend configuration from preflight discovery evidence.
    #[must_use]
    pub fn builder(discovery: m80_preflight::Discovery) -> BackendConfigBuilder {
        BackendConfigBuilder {
            run_root: discovery.run_root.clone(),
            discovery,
            max_concurrent_vms: 8,
            jail_uid: 3000,
            jail_gid: 3000,
            cgroup_mode: CgroupMode::UnifiedV2,
        }
    }

    /// Discovery output used by this backend.
    #[must_use]
    pub fn discovery(&self) -> &m80_preflight::Discovery {
        &self.discovery
    }

    /// Maximum concurrent VMs admitted on this host.
    #[must_use]
    pub fn max_concurrent_vms(&self) -> u32 {
        self.max_concurrent_vms
    }

    /// Per-process run-root directory.
    #[must_use]
    pub fn run_root(&self) -> &Path {
        &self.run_root
    }

    /// UID for the jailed firecracker process.
    #[must_use]
    pub fn jail_uid(&self) -> u32 {
        self.jail_uid
    }

    /// GID for the jailed firecracker process.
    #[must_use]
    pub fn jail_gid(&self) -> u32 {
        self.jail_gid
    }

    /// Cgroup mode (unified-v2 or disabled).
    #[must_use]
    pub fn cgroup_mode(&self) -> CgroupMode {
        self.cgroup_mode
    }
}

/// Builder for [`BackendConfig`].
#[derive(Debug, Clone)]
pub struct BackendConfigBuilder {
    discovery: m80_preflight::Discovery,
    max_concurrent_vms: u32,
    run_root: PathBuf,
    jail_uid: u32,
    jail_gid: u32,
    cgroup_mode: CgroupMode,
}

impl BackendConfigBuilder {
    /// Set the maximum concurrent VM admission limit.
    #[must_use]
    pub fn max_concurrent_vms(mut self, max: u32) -> Self {
        self.max_concurrent_vms = max;
        self
    }

    /// Set the backend run-root directory.
    #[must_use]
    pub fn run_root(mut self, run_root: impl Into<PathBuf>) -> Self {
        self.run_root = run_root.into();
        self
    }

    /// Set the UID for the jailed firecracker process.
    #[must_use]
    pub fn jail_uid(mut self, uid: u32) -> Self {
        self.jail_uid = uid;
        self
    }

    /// Set the GID for the jailed firecracker process.
    #[must_use]
    pub fn jail_gid(mut self, gid: u32) -> Self {
        self.jail_gid = gid;
        self
    }

    /// Set the cgroup hardening mode.
    #[must_use]
    pub fn cgroup_mode(mut self, mode: CgroupMode) -> Self {
        self.cgroup_mode = mode;
        self
    }

    /// Finish building the backend configuration.
    #[must_use]
    pub fn build(self) -> BackendConfig {
        BackendConfig {
            discovery: self.discovery,
            max_concurrent_vms: self.max_concurrent_vms,
            run_root: self.run_root,
            jail_uid: self.jail_uid,
            jail_gid: self.jail_gid,
            cgroup_mode: self.cgroup_mode,
        }
    }
}

/// Cgroup hardening mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub enum CgroupMode {
    /// Cgroup v2 unified hierarchy enforcement.
    UnifiedV2,
    /// No cgroup hardening.
    Disabled,
}

/// One stdout/stderr chunk observed from [`RunningSandbox::exec_streaming`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecChunk {
    /// Standard-output bytes with the per-stream sequence number assigned by
    /// guestd.
    Stdout {
        /// Monotonic sequence number within stdout for one request.
        seq: u32,
        /// Raw stdout bytes.
        bytes: Vec<u8>,
    },
    /// Standard-error bytes with the per-stream sequence number assigned by
    /// guestd.
    Stderr {
        /// Monotonic sequence number within stderr for one request.
        seq: u32,
        /// Raw stderr bytes.
        bytes: Vec<u8>,
    },
}

/// Host-originated event sent to a running PTY session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PtyHostEvent {
    /// Raw terminal input bytes.
    Input(Vec<u8>),
    /// Terminal size changed.
    Resize(m80_proto::PtySize),
    /// Terminal control event such as EOF or a wrapper signal.
    Control(m80_proto::PtyControlEvent),
    /// Cancel the in-flight PTY session.
    Cancel,
}

/// One terminal-output chunk observed from [`RunningSandbox::exec_pty`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PtyOutputChunk {
    /// Monotonic sequence number assigned by guestd.
    pub seq: u32,
    /// Raw merged terminal output bytes.
    pub bytes: Vec<u8>,
}

/// Snapshot of the merged configuration with each field tagged by source.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EffectiveConfig {
    /// One row per resolved field.
    pub fields: Vec<EffectiveField>,
}

/// One row in [`EffectiveConfig`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EffectiveField {
    /// Field name (e.g., `"max_concurrent_vms"`).
    pub name: String,
    /// Effective value rendered as a string.
    pub value: String,
    /// Source that supplied the value.
    pub source: ConfigSource,
}

/// Where one [`EffectiveField`] value came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub enum ConfigSource {
    /// Built-in default.
    Default,
    /// `/etc/m80/config.toml`.
    SystemFile,
    /// `/etc/m80/config.d/*.toml`.
    SystemDropIn,
    /// `~/.config/m80/config.toml`.
    UserFile,
    /// `~/.config/m80/config.d/*.toml`.
    UserDropIn,
    /// `M80_*` environment variable.
    Env,
    /// Command-line flag.
    Flag,
}

/// Per-VM launch parameters: resource sizing, workspace path, network policy,
/// idle-timeout deadline, declared pmem layers, and one-shot mode. Built once
/// per [`Sandbox`] and cloned into warm-pool slots. `workspace` must be `None`
/// for pool slots. Fields that accept `None` resolve to their defaults at
/// launch time (e.g., 1 vCPU, 512 MiB RAM, 5-minute idle timeout, no
/// preallocated drives). An empty `pmem_layers` vector preserves the default
/// v0.1 launch behavior; non-empty vectors are validated before any run-dir or
/// VMM side effect, resolved to read-only host backings according to their
/// sharing policy, attached through Firecracker pmem, and mounted by guestd
/// before the running handle is returned.
///
/// Current cold launches do not ask Firecracker's official jailer for a new PID
/// namespace; a compromised VMM that forks a child before host teardown is
/// contained by cgroup/process cleanup rather than PID-namespace lifetime.
#[derive(Debug, Clone)]
pub struct SandboxConfig {
    /// Optional caller-provided VM id; auto-derived when absent.
    pub vm_id: Option<String>,
    /// Optional host directory hydrated into a scratch ext4 inside the VM.
    pub workspace: Option<PathBuf>,
    /// Caller intent for network egress.
    pub network: NetworkPolicy,
    /// Number of vCPUs (default: 1).
    pub vcpu_count: Option<u32>,
    /// Memory in MiB (default: 512).
    pub mem_size_mib: Option<u32>,
    /// Optional cgroup v2 CPU set for this VM, e.g. `0-3` or `0,2`.
    ///
    /// `None` inherits the parent cgroup's effective CPU set. When set and
    /// cgroup mode is `UnifiedV2`, launch writes the value to the VM leaf
    /// `cpuset.cpus` before enrolling Firecracker.
    pub cpuset_cpus: Option<String>,
    /// Optional Firecracker CPU template.
    ///
    /// The default is `None`, which exposes the host CPU surface directly and
    /// optimizes same-host launch/restore latency. Callers that need AWS
    /// template masking for cross-host snapshot portability must opt in.
    pub cpu_template: Option<CpuTemplate>,
    /// Optional Firecracker native logger verbosity.
    ///
    /// `None` uses m80's default `Warning` threshold while still enabling the
    /// logger. Set a concrete level to capture more or less Firecracker-native
    /// device/VMM detail in the jail-root `firecracker.log`.
    pub fc_log_level: Option<FcLogLevel>,
    /// Optional host-cache policy override for writable preboot drives.
    ///
    /// `None` uses m80's ephemeral default: `CacheType::Unsafe` for the
    /// rootfs overlay and workspace scratch drives. Set `Some(Writeback)` only
    /// when guest writes must retain Firecracker's conservative host sync
    /// behavior.
    pub drive_cache_type: Option<CacheType>,
    /// Boot args appended to the kernel command line.
    pub boot_args: Option<String>,
    /// Sparse overlay size in bytes (default: 512 MiB).
    ///
    /// The overlay is allocated as a sparse file at launch time and formatted
    /// with `mkfs.ext4 -F`. Cost at creation is ~0 bytes on disk; it grows
    /// as the guest writes. See `docs/design/storage-overlay.md §5`.
    ///
    /// 512 MiB is large enough to hold typical agent workspace output (source
    /// trees, build artefacts, generated files) without risking "disk full"
    /// errors during normal use, while remaining a sparse reservation that
    /// costs nothing until the guest actually writes. Callers with larger or
    /// smaller needs should override this field explicitly.
    pub overlay_size_bytes: u64,
    /// How the empty overlay template is cloned into each per-VM overlay.
    ///
    /// The default is `ByteCopy`, which is deterministic on ext4, tmpfs, XFS,
    /// btrfs, and ZFS. Set `Reflink` to require CoW clone semantics, or `Auto`
    /// to probe the run-root filesystem once and select `Reflink` or
    /// `ByteCopy` before the clone command is built. A selected clone mode is
    /// fail-closed; m80 does not retry another mode after `cp` fails.
    pub overlay_clone_mode: OverlayTemplateCloneMode,
    /// How long the VM may sit idle (no exec in flight, none pending) before
    /// the host issues a graceful shutdown. `None` opts out of idle shutdown.
    ///
    /// Default: `Some(Duration::from_secs(300))` (5 minutes).
    ///
    /// The watcher thread resets the deadline after each successful `exec`
    /// call. If the deadline elapses with no exec activity, the next `exec`
    /// call returns `FcError::IdleTimedOut`; the caller should then drop or
    /// `stop()` the sandbox. `None` disables the watcher entirely — the VM
    /// runs until the caller explicitly stops it.
    pub idle_timeout: Option<Duration>,
    /// Ask the official Firecracker jailer to double-fork before exec'ing
    /// Firecracker. The API socket remains the management surface; m80 records
    /// no live jailer parent PID for daemonized launches.
    pub daemonize: bool,
    /// Opaque request id associated with the launch/lifecycle owner of this
    /// sandbox. CLI callers set this once per invocation so diagnostics and
    /// guest stderr can be grepped with the same token.
    pub request_id: Option<String>,
    /// Declared read-only erofs-over-pmem layers for this VM.
    ///
    /// The vector defaults to empty. Non-empty values validate, resolve to
    /// per-VM backings during storage prep, bind read-only into the jail,
    /// attach through `PUT /pmem/{id}`, and mount as erofs+DAX inside the guest
    /// before `launch()` returns `RunningSandbox`.
    pub pmem_layers: Vec<PmemLayer>,
    /// Number of writable placeholder drive slots created before
    /// `InstanceStart` so later attachment can use `PATCH /drives/{id}`.
    ///
    /// Values above zero keep additional live virtio-blk devices attached for
    /// the VM lifetime. That removes hotplug latency for later tenant-drive
    /// attach, but each idle slot is still Firecracker block-device surface.
    pub preallocated_drive_slots: u8,
    /// Destroy-after-use mode for conveyor-belt callers. The first user exec
    /// consumes the VM for reuse; later exec attempts fail typed.
    pub one_shot: bool,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        SandboxConfig {
            vm_id: None,
            workspace: None,
            network: NetworkPolicy::NoEgress,
            vcpu_count: None,
            mem_size_mib: None,
            cpuset_cpus: None,
            cpu_template: None,
            fc_log_level: None,
            drive_cache_type: None,
            boot_args: None,
            overlay_size_bytes: 512 * 1024 * 1024,
            overlay_clone_mode: OverlayTemplateCloneMode::ByteCopy,
            idle_timeout: Some(Duration::from_secs(300)),
            daemonize: false,
            request_id: None,
            pmem_layers: Vec::new(),
            preallocated_drive_slots: DEFAULT_PREALLOCATED_DRIVE_SLOTS,
            one_shot: false,
        }
    }
}

/// A sandbox in `Created` state: admission permit acquired from the backend
/// semaphore, configuration validated, no Firecracker process started yet.
/// Advancing to `Running` requires calling a launch method (cold-boot or
/// snapshot restore); if launch fails the permit is released via `Drop`.
/// Callers cannot construct this directly — use [`Backend::admit`].
pub struct Sandbox {
    /// Per-VM configuration.
    pub(crate) config: SandboxConfig,
    /// Admission permit; dropped if launch fails.
    pub(crate) permit: AdmissionPermit,
    /// Reference to the backend that created this sandbox.
    pub(crate) backend: Arc<Backend>,
    /// Delete partial run-dir on launch failure instead of preserving it.
    pub(crate) delete_run_dir_on_launch_error: bool,
}

impl std::fmt::Debug for Sandbox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Sandbox")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

/// Fallback cleanup guard for a running Firecracker VM.
///
/// Dropped automatically when a `RunningSandbox` is abandoned without calling
/// `stop()` or `force_kill()` — e.g., on an error path in the caller. Sends
/// SIGKILL to the Firecracker and jailer processes and unmounts any snapshot
/// bind-mount to avoid leaking processes and mount points.
///
/// `stop()` and `force_kill()` disarm the guard by calling
/// [`ForceKillGuard::disarm`] before returning; their own cleanup supersedes
/// this one.
pub(crate) struct ForceKillGuard {
    pub(crate) vm_id: String,
    pub(crate) firecracker_pid: u32,
    pub(crate) jailer_pid: u32,
    pub(crate) watcher_stop: Arc<std::sync::atomic::AtomicBool>,
    pub(crate) snapshot_mount: Option<PathBuf>,
    armed: bool,
}

impl ForceKillGuard {
    pub(crate) fn new(
        vm_id: String,
        firecracker_pid: u32,
        jailer_pid: u32,
        watcher_stop: Arc<std::sync::atomic::AtomicBool>,
        snapshot_mount: Option<PathBuf>,
    ) -> Self {
        Self {
            vm_id,
            firecracker_pid,
            jailer_pid,
            watcher_stop,
            snapshot_mount,
            armed: true,
        }
    }

    /// Disarm the guard so Drop is a no-op.
    pub(crate) fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for ForceKillGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        tracing::warn!(vm_id = %self.vm_id, "RunningSandbox dropped without stop/force_kill — force-killing");
        self.watcher_stop
            .store(true, std::sync::atomic::Ordering::Relaxed);
        // Best-effort SIGKILL; errors logged and swallowed because Drop must not panic.
        if let Err(e) = crate::lifecycle::kill_and_reap_pid(self.firecracker_pid) {
            tracing::error!(vm_id = %self.vm_id, error = %e, "Drop force-kill firecracker failed");
        }
        if self.jailer_pid != self.firecracker_pid {
            if let Err(e) = crate::lifecycle::kill_and_reap_pid(self.jailer_pid) {
                tracing::error!(vm_id = %self.vm_id, error = %e, "Drop force-kill jailer failed");
            }
        }
        if let Some(snap) = self.snapshot_mount.take() {
            crate::lifecycle::unmount_snapshot_bind(Some(&snap));
        }
    }
}

/// A sandbox in `Running` state — VM booted, vsock ready, ready to accept
/// exec requests. All fields are `pub(crate)` — callers use the typed
/// methods on `impl RunningSandbox` (`vm_id`, `exec`, `stop`, `force_kill`).
///
/// Concurrent exec on one persistent VM is intentionally not part of the
/// contract. `exec` requires `&mut self`, so an `Arc<RunningSandbox>` cannot
/// be cloned into threads and used for parallel exec calls:
///
/// ```compile_fail
/// use std::sync::Arc;
///
/// use m80_firecracker::RunningSandbox;
/// use m80_proto::ExecRequest;
///
/// fn cannot_exec_from_shared_arc(running: Arc<RunningSandbox>, req: ExecRequest) {
///     let running = Arc::clone(&running);
///     std::thread::spawn(move || {
///         let _ = running.exec(req);
///     });
/// }
/// ```
pub struct RunningSandbox {
    /// VM identifier.
    pub(crate) vm_id: String,
    /// Opaque request id for launch/stop diagnostics when one was supplied by
    /// the caller.
    pub(crate) request_id: Option<String>,
    /// Per-VM run directory.
    pub(crate) run_dir: PathBuf,
    /// The materialized jailer chroot.
    pub(crate) jail: MaterializedJail,
    /// Active-use markers for shared pmem image-store artifacts.
    pub(crate) shared_pmem_refs: Vec<m80_image_store::SharedImageRef>,
    /// Cgroup subtree (Some if UnifiedV2 mode).
    pub(crate) cgroup: Option<m80_cgroup::Subtree>,
    /// Per-VM rootfs clone.
    pub(crate) rootfs: Rootfs,
    /// Scratch image (Some if workspace is configured).
    pub(crate) scratch: Option<Scratch>,
    /// Snapshot directory bind-mounted into the jail for restore, if any.
    pub(crate) snapshot_mount: Option<PathBuf>,
    /// REST client to the Firecracker process.
    pub(crate) client: Client,
    /// Live firecracker + jailer pids.
    pub(crate) firecracker: JailedFirecracker,
    /// Admission permit; held for the lifetime of this sandbox.
    pub(crate) permit: AdmissionPermit,
    /// Per-run-dir ownership lock; held until the run-dir is deleted or
    /// preserved so same-vm_id launches cannot reuse live state.
    pub(crate) lease_guard: LeaseGuard,
    /// Reference to the backend.
    pub(crate) backend: Arc<Backend>,
    /// Monotonic timestamp (nanos since an arbitrary epoch) of the last
    /// `exec` activity. Updated at the start and end of every `exec` call.
    /// Written with `Relaxed` ordering — the watcher only needs a recent
    /// value; happens-before precision is not required.
    pub(crate) last_activity_ns: Arc<AtomicU64>,
    /// Number of exec requests currently in flight.
    pub(crate) active_execs: Arc<AtomicUsize>,
    /// Watcher sets this flag when the idle deadline expires.
    /// `exec` checks it at entry and returns `FcError::IdleTimedOut`.
    pub(crate) idle_timed_out: Arc<std::sync::atomic::AtomicBool>,
    /// Signal from `stop` / `force_kill` to the watcher thread to exit.
    pub(crate) watcher_stop: Arc<std::sync::atomic::AtomicBool>,
    /// Watcher thread join handle (`None` when `idle_timeout` is `None`).
    pub(crate) watcher_thread: Option<std::thread::JoinHandle<()>>,
    /// Optional diagnostics writer for `<run_dir>/diagnostics.jsonl`.
    pub(crate) diagnostics: Option<m80_observability::Diagnostics>,
    /// Number of preallocated hotplug slots PUT before instance start.
    pub(crate) preallocated_drive_slots: u8,
    pub(crate) one_shot: bool,
    pub(crate) one_shot_consumed: bool,
    /// Fallback cleanup guard; disarmed by `stop()` and `force_kill()` before
    /// they perform their own teardown, so Drop is a no-op on the happy path.
    pub(crate) kill_guard: ForceKillGuard,
    /// Whether this VM owns outbound-network residue that delete must reap.
    pub(crate) network_cleanup: bool,
}

impl RunningSandbox {
    /// Return the per-VM run directory.
    pub fn run_dir(&self) -> &Path {
        &self.run_dir
    }

    /// Return the host-visible Firecracker native metrics file path.
    pub fn fc_metrics_path(&self) -> PathBuf {
        Self::fc_metrics_path_for_jail_root(self.jail.jail_root())
    }

    fn fc_metrics_path_for_jail_root(jail_root: &Path) -> PathBuf {
        crate::layout::fc_metrics_path_from_jail_root(jail_root)
    }
}

impl std::fmt::Debug for RunningSandbox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RunningSandbox")
            .field("vm_id", &self.vm_id)
            .field("run_dir", &self.run_dir)
            .finish_non_exhaustive()
    }
}

/// A sandbox in `Stopped` state — VM exited, scratch image quiesced,
/// run-dir intact. All fields are `pub(crate)`; callers use the typed
/// methods on `impl StoppedSandbox`.
pub struct StoppedSandbox {
    /// VM identifier.
    pub(crate) vm_id: String,
    /// Opaque request id for stop/delete diagnostics when one was supplied by
    /// the caller.
    pub(crate) request_id: Option<String>,
    /// Per-VM run directory.
    pub(crate) run_dir: PathBuf,
    /// Scratch image (for `extract_changes`; consumed when extracted).
    pub(crate) scratch: Option<Scratch>,
    /// Admission permit. Held purely for its `Drop` side-effect — when
    /// `StoppedSandbox` is consumed by `delete()` or
    /// `preserve_for_triage()`, this field is dropped and the permit is
    /// returned to the semaphore.
    #[allow(dead_code)]
    pub(crate) permit: AdmissionPermit,
    /// Per-run-dir ownership lock carried from launch through delete/preserve.
    #[allow(dead_code)]
    pub(crate) lease_guard: LeaseGuard,
    /// Run-root path (needed for `preserve_for_triage`).
    pub(crate) run_root: PathBuf,
    /// Optional diagnostics writer carried across Running -> Stopped.
    pub(crate) diagnostics: Option<m80_observability::Diagnostics>,
    /// Whether delete must reap outbound-network residue before run-dir removal.
    pub(crate) network_cleanup: bool,
    /// Privileged outbound-network helper used after the backend handle is gone.
    pub(crate) network_helper: Arc<NetworkHelperClient>,
}

impl std::fmt::Debug for StoppedSandbox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StoppedSandbox")
            .field("vm_id", &self.vm_id)
            .field("run_dir", &self.run_dir)
            .finish_non_exhaustive()
    }
}

/// Bundle of per-VM storage resources produced by the storage-prep phase.
pub(crate) struct StoragePrep {
    /// The per-VM rootfs clone.
    pub(crate) rootfs: Rootfs,
    /// The scratch image (Some if a workspace was requested).
    pub(crate) scratch: Option<Scratch>,
    /// Writable placeholder drive images bound into the jail for preallocated
    /// hotplug slots.
    pub(crate) preallocated_drive_slots: Vec<PathBuf>,
    /// Pmem backing files resolved from the image store.
    pub(crate) pmem_backings: Vec<ResolvedPmemBacking>,
    /// Active-use markers acquired for shared pmem backings.
    pub(crate) shared_pmem_refs: Vec<m80_image_store::SharedImageRef>,
}

/// Host-side pmem backing prepared for one declared layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedPmemBacking {
    /// Host backing path; per-VM sharing uses the run directory, shared
    /// sharing uses the canonical image-store artifact.
    pub(crate) host_path: PathBuf,
    /// Deterministic jail basename, generated from the slot index.
    pub(crate) jail_basename: String,
    /// Sharing policy that produced this backing.
    pub(crate) sharing: crate::pmem::PmemSharing,
}

impl std::fmt::Debug for StoragePrep {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StoragePrep")
            .field("has_scratch", &self.scratch.is_some())
            .field(
                "preallocated_drive_slots",
                &self.preallocated_drive_slots.len(),
            )
            .field("pmem_backings", &self.pmem_backings.len())
            .field("shared_pmem_refs", &self.shared_pmem_refs.len())
            .finish()
    }
}

/// Resolved network state after phase 6.
#[derive(Debug)]
pub(crate) enum RealizedNetwork {
    /// No NIC configured; iptables untouched.
    NoEgress,
    /// Firecracker will attach a TAP in an m80-owned VMM namespace.
    OutboundNat {
        /// TAP device name visible inside the VMM namespace.
        tap_name: String,
        /// Host path to the m80-owned VMM network namespace.
        vmm_netns_path: PathBuf,
        /// Guest MAC address assigned during network planning.
        guest_mac: String,
    },
    /// Firecracker will join a caller-provided network namespace.
    JoinNetns {
        /// Namespace path supplied by the caller.
        netns_path: PathBuf,
        /// Caller-created TAP device visible inside the namespace.
        tap_name: String,
        /// Guest MAC address assigned during network planning.
        guest_mac: String,
        /// Guest IPv4 address with prefix configured by PID 1.
        guest_ipv4: String,
        /// Default gateway configured by PID 1.
        gateway_ipv4: String,
        /// DNS resolvers written by PID 1.
        dns_resolvers: Vec<String>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn running_sandbox_fc_metrics_path_uses_jail_root_layout() {
        let jail_root = Path::new("/tmp/m80-run/vm/firecracker/vm/root");

        assert_eq!(
            RunningSandbox::fc_metrics_path_for_jail_root(jail_root),
            jail_root.join(crate::layout::FIRECRACKER_METRICS)
        );
    }
}
