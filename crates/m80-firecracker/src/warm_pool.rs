//! Warm-pool allocator built on top of snapshot restore.

mod cpu_allocator;
mod fill_worker;
mod lease;

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use m80_proto::{ExecRequest, ExecStatus};
use m80_snapshot::{verify_snapshot_manifest, SnapshotPaths};

use crate::error::{ConfigError, FcError};
use crate::types::{Backend, RunningSandbox, SandboxConfig};

use cpu_allocator::build_warm_pool_cpu_ranges;
pub use cpu_allocator::WarmPoolCpuAllocator;
use fill_worker::spawn_fill_worker;
pub use lease::WarmLease;

const RESTORED_SLOT_SETTLE: Duration = Duration::from_secs(1);

/// Maximum number of concurrent background fill workers per pool.
///
/// Prevents runaway thread spawning when `target_ready` is high or when the
/// admit semaphore is the bottleneck. Additional fill demand beyond this cap is
/// satisfied by existing workers completing and re-triggering.
const MAX_FILL_THREADS: usize = 4;

/// Exponential-backoff delays applied to background fill retries after a
/// consecutive run of slot-launch failures.
///
/// Indexed by `min(consecutive_fill_errors - 1, LEN - 1)`:
///   errors=1 → 50 ms, errors=2 → 200 ms, errors=3 → 1 s, errors≥4 → 10 s.
const FILL_BACKOFF: &[Duration] = &[
    Duration::from_millis(50),
    Duration::from_millis(200),
    Duration::from_secs(1),
    Duration::from_secs(10),
];

/// Parameters that govern a [`WarmPool`]: how many slots to keep ready,
/// which snapshot pair to restore from, and what probe must pass before a
/// slot is considered healthy. All fields are required at construction;
/// `target_ready` must be greater than zero and `sandbox.workspace` must be
/// `None` — warm slots are stateless and may not carry a workspace.
#[derive(Debug, Clone)]
pub struct WarmPoolConfig {
    /// Number of ready slots the pool tries to keep filled.
    pub target_ready: usize,
    /// Snapshot pair used to restore every clean slot.
    pub snapshot: SnapshotPaths,
    /// Sandbox configuration applied to each restored slot.
    pub sandbox: SandboxConfig,
    /// Probe request that must complete successfully before a restored slot
    /// enters `Ready`.
    pub ready_probe: ExecRequest,
    /// Prefix for generated per-slot VM ids.
    pub vm_id_prefix: String,
    /// Optional CPU range allocator for per-slot cgroup `cpuset.cpus`.
    pub cpu_allocator: Option<WarmPoolCpuAllocator>,
}

/// Observable warm-pool state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WarmPoolSnapshot {
    /// Configured ready-slot target.
    pub target_ready: usize,
    /// Slots ready to lease now.
    pub ready: usize,
    /// Restore operations currently filling a slot.
    pub filling: usize,
    /// Slots currently handed to callers.
    pub leased: usize,
    /// Slots discarded since pool creation.
    pub discarded: usize,
}

/// A pool of pre-restored Firecracker VMs ready to serve exec requests with
/// minimal latency. Each slot is created by restoring a snapshot and running a
/// ready-probe; only slots that pass the probe enter the ready queue. Slots are
/// leased one at a time via [`WarmPool::try_lease`] and are discarded (not
/// returned) when the lease is dropped. On `Drop`, the pool sets the shutdown
/// flag and force-kills all remaining ready slots to avoid leaking processes.
pub struct WarmPool {
    inner: Arc<WarmPoolInner>,
}

struct WarmPoolInner {
    backend: Arc<Backend>,
    config: WarmPoolConfig,
    state: Mutex<WarmPoolState>,
    changed: Condvar,
    shutdown: AtomicBool,
    next_slot: AtomicU64,
}

struct WarmPoolState {
    ready: VecDeque<WarmSlot>,
    filling: usize,
    leased: usize,
    discarded: usize,
    free_cpuset_cpus: VecDeque<String>,
    /// The most recent slot-fill failure message, or `None` if the last fill
    /// succeeded (or no fill has been attempted).
    ///
    /// # Staleness limitation
    ///
    /// This field is set when a fill attempt fails and cleared when a fill
    /// attempt succeeds. It is NOT cleared by the passage of time. If no fill
    /// has been attempted for an extended period — for example because
    /// `target_ready` was reduced to zero, the pool is shutting down, or all
    /// background fill workers have exited — the stored message may refer to an
    /// error that occurred seconds or minutes ago and no longer reflects the
    /// current state of the pool.
    ///
    /// `wait_for_ready` reports this value on timeout, so a caller may see a
    /// stale error message that predates the timeout window. A future revision
    /// (bead m80-r5-future) could add a timestamp alongside the message to let
    /// callers distinguish "failure within the last N seconds" from an old
    /// failure. For now, treat the value as "the most recent failure since last
    /// success", not "the current state of the pool".
    last_fill_error: Option<String>,
    /// Number of consecutive slot-launch failures; reset to 0 on success.
    /// Used to index into `FILL_BACKOFF` to throttle retry threads.
    consecutive_fill_errors: u32,
}

pub(super) struct WarmSlot {
    sandbox: RunningSandbox,
    cpuset_cpus: Option<String>,
}

impl WarmPool {
    /// Create a warm pool. Call [`fill_to_target_blocking`] before allocating
    /// if the first request must be served from a pre-filled slot.
    pub fn new(backend: Arc<Backend>, config: WarmPoolConfig) -> Result<Self, FcError> {
        if config.target_ready == 0 {
            return Err(FcError::Config(ConfigError::InvalidValue {
                field: "warm_pool.target_ready",
                reason: "must be > 0".into(),
            }));
        }
        if config.sandbox.workspace.is_some() {
            return Err(FcError::Config(ConfigError::InvalidValue {
                field: "warm_pool.sandbox.workspace",
                reason: "warm pool slots must be stateless: SandboxConfig::workspace must be None"
                    .into(),
            }));
        }
        let free_cpuset_cpus =
            build_warm_pool_cpu_ranges(config.target_ready, config.cpu_allocator)?
                .into_iter()
                .collect();
        Ok(WarmPool {
            inner: Arc::new(WarmPoolInner {
                backend,
                config,
                state: Mutex::new(WarmPoolState {
                    ready: VecDeque::new(),
                    filling: 0,
                    leased: 0,
                    discarded: 0,
                    free_cpuset_cpus,
                    last_fill_error: None,
                    consecutive_fill_errors: 0,
                }),
                changed: Condvar::new(),
                shutdown: AtomicBool::new(false),
                next_slot: AtomicU64::new(0),
            }),
        })
    }

    /// Restore slots synchronously until `target_ready` ready slots exist.
    pub fn fill_to_target_blocking(&self) -> Result<(), FcError> {
        loop {
            if self.inner.shutdown.load(Ordering::Relaxed) {
                return Ok(());
            }
            {
                let state = self.inner.state.lock().unwrap_or_else(|p| p.into_inner());
                if state.ready.len() >= self.inner.config.target_ready {
                    return Ok(());
                }
            }
            let cpuset_cpus = {
                let mut state = self.inner.state.lock().unwrap_or_else(|p| p.into_inner());
                state.reserve_cpuset_cpus()
            };
            if self.inner.config.cpu_allocator.is_some() && cpuset_cpus.is_none() {
                return Ok(());
            }
            let slot = match self.inner.launch_slot(cpuset_cpus.clone()) {
                Ok(slot) => slot,
                Err(err) => {
                    let mut state = self.inner.state.lock().unwrap_or_else(|p| p.into_inner());
                    state.release_cpuset_cpus(cpuset_cpus);
                    return Err(err);
                }
            };
            let mut state = self.inner.state.lock().unwrap_or_else(|p| p.into_inner());
            state.ready.push_back(slot);
            state.last_fill_error = None;
            self.inner.changed.notify_all();
        }
    }

    /// Start background restore workers until the ready target is covered by
    /// ready plus filling slots.
    pub fn start_background_fill(&self) {
        self.inner.start_background_fill();
    }

    /// Lease one ready slot. Empty-pool behavior is fail-closed: this never
    /// cold-boots or restores synchronously on the request path.
    pub fn try_lease(&self) -> Result<WarmLease, FcError> {
        let sandbox = {
            let mut state = self.inner.state.lock().unwrap_or_else(|p| p.into_inner());
            let Some(sandbox) = state.ready.pop_front() else {
                return Err(FcError::PoolEmpty {
                    target_ready: self.inner.config.target_ready,
                });
            };
            state.leased += 1;
            sandbox
        };
        self.inner.start_background_fill();
        Ok(WarmLease::new(sandbox, Arc::clone(&self.inner)))
    }

    /// Wait until at least `min_ready` slots are ready.
    pub fn wait_for_ready(&self, min_ready: usize, timeout: Duration) -> Result<(), FcError> {
        let deadline = Instant::now() + timeout;
        let mut state = self.inner.state.lock().unwrap_or_else(|p| p.into_inner());
        loop {
            if state.ready.len() >= min_ready {
                return Ok(());
            }
            let now = Instant::now();
            if now >= deadline {
                if let Some(err) = &state.last_fill_error {
                    return Err(FcError::WarmPoolFillFailed {
                        detail: err.clone(),
                    });
                }
                return Err(FcError::PoolEmpty {
                    target_ready: self.inner.config.target_ready,
                });
            }
            let wait = deadline.saturating_duration_since(now);
            let (next, _) = self
                .inner
                .changed
                .wait_timeout(state, wait)
                .unwrap_or_else(|p| p.into_inner());
            state = next;
        }
    }

    /// Wait until no fill workers are running (`filling == 0`). Used by the
    /// drain path so in-flight slot restores complete before the pool is dropped.
    pub fn wait_for_idle(&self, timeout: Duration) -> Result<(), FcError> {
        let deadline = Instant::now() + timeout;
        let mut state = self.inner.state.lock().unwrap_or_else(|p| p.into_inner());
        loop {
            if state.filling == 0 {
                return Ok(());
            }
            let now = Instant::now();
            if now >= deadline {
                return Err(FcError::WarmOwnerDrainTimeout { timeout });
            }
            let wait = deadline.saturating_duration_since(now);
            let (next, _) = self
                .inner
                .changed
                .wait_timeout(state, wait)
                .unwrap_or_else(|p| p.into_inner());
            state = next;
        }
    }

    /// Return an observable state snapshot.
    #[must_use]
    pub fn snapshot(&self) -> WarmPoolSnapshot {
        self.inner.snapshot()
    }
}

impl Drop for WarmPool {
    fn drop(&mut self) {
        self.inner.shutdown.store(true, Ordering::Relaxed);
        loop {
            let ready = {
                let mut state = self.inner.state.lock().unwrap_or_else(|p| p.into_inner());
                if state.ready.is_empty() && state.filling == 0 {
                    self.inner.changed.notify_all();
                    return;
                }
                while state.ready.is_empty() && state.filling > 0 {
                    let (next, _) = self
                        .inner
                        .changed
                        .wait_timeout(state, Duration::from_millis(50))
                        .unwrap_or_else(|p| p.into_inner());
                    state = next;
                }
                state.ready.drain(..).collect::<Vec<_>>()
            };
            for slot in ready {
                let _ = discard_sandbox(slot.sandbox);
            }
        }
    }
}

impl WarmPoolInner {
    fn launch_slot(&self, cpuset_cpus: Option<String>) -> Result<WarmSlot, FcError> {
        verify_warm_snapshot(
            &self.config.snapshot,
            &self
                .backend
                .config
                .discovery
                .manifest
                .expected_firecracker_version,
        )?;
        let slot_id = self.next_slot.fetch_add(1, Ordering::Relaxed);
        let mut sandbox_config = self.config.sandbox.clone();
        sandbox_config.vm_id = Some(format!("{}-{slot_id}", self.config.vm_id_prefix));
        sandbox_config.cpuset_cpus = cpuset_cpus.clone();
        let sandbox = self.backend.admit(sandbox_config)?;
        let mut running = sandbox
            .launch_from_snapshot(self.config.snapshot.clone(), &self.backend.config.discovery)?;
        run_ready_probe(&mut running, &self.config.ready_probe)?;
        Ok(WarmSlot {
            sandbox: running,
            cpuset_cpus,
        })
    }

    fn start_background_fill(self: &Arc<Self>) {
        loop {
            {
                let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
                if self.shutdown.load(Ordering::Relaxed) {
                    return;
                }
                // Stop if the pool is already at target or already has the
                // maximum number of concurrent fill workers running.
                let deficit = self
                    .config
                    .target_ready
                    .saturating_sub(state.ready.len() + state.filling);
                if deficit == 0 || state.filling >= MAX_FILL_THREADS {
                    return;
                }
                let cpuset_cpus = state.reserve_cpuset_cpus();
                if self.config.cpu_allocator.is_some() && cpuset_cpus.is_none() {
                    return;
                }
                state.filling += 1;
                drop(state);

                let inner = Arc::clone(self);
                spawn_fill_worker(inner, cpuset_cpus);
            }
        }
    }

    fn lease_finished(&self, cpuset_cpus: Option<String>) {
        {
            let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
            state.leased = state.leased.saturating_sub(1);
            state.discarded += 1;
            state.release_cpuset_cpus(cpuset_cpus);
            self.changed.notify_all();
        }
    }

    fn snapshot(&self) -> WarmPoolSnapshot {
        let state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        WarmPoolSnapshot {
            target_ready: self.config.target_ready,
            ready: state.ready.len(),
            filling: state.filling,
            leased: state.leased,
            discarded: state.discarded,
        }
    }
}

impl WarmPoolState {
    fn reserve_cpuset_cpus(&mut self) -> Option<String> {
        self.free_cpuset_cpus.pop_front()
    }

    fn release_cpuset_cpus(&mut self, cpuset_cpus: Option<String>) {
        if let Some(cpuset_cpus) = cpuset_cpus {
            self.free_cpuset_cpus.push_back(cpuset_cpus);
        }
    }
}

fn discard_sandbox(sandbox: RunningSandbox) -> Result<(), FcError> {
    sandbox.force_kill()?.delete()
}

fn verify_warm_snapshot(
    paths: &SnapshotPaths,
    expected_firecracker_version: &str,
) -> Result<(), FcError> {
    verify_snapshot_manifest(paths, expected_firecracker_version)
        .map(|_| ())
        .map_err(FcError::Snapshot)
}

/// Discard a sandbox, first recording a diagnostics event if `exec_err` is
/// `Some` (i.e., we are discarding after an exec failure, not normal teardown).
pub(crate) fn discard_sandbox_with_diagnostics(
    mut sandbox: RunningSandbox,
    exec_err: Option<&FcError>,
) -> Result<(), FcError> {
    if let Some(err) = exec_err {
        let vm_id = sandbox.vm_id.clone();
        let request_id = sandbox.request_id.clone();
        crate::diagnostics::record_owned(
            &mut sandbox.diagnostics,
            m80_observability::Phase::Stop,
            &vm_id,
            request_id.as_deref(),
            &format!("one-shot exec failed; discarding VM: {err}"),
        );
    }
    sandbox.force_kill()?.delete()
}

fn run_ready_probe(sandbox: &mut RunningSandbox, req: &ExecRequest) -> Result<(), FcError> {
    let mut last_error = None;
    for _ in 0..5 {
        match sandbox.exec_ready_probe(req.clone()) {
            Ok(resp) if resp.status == ExecStatus::Completed && resp.exit_code == Some(0) => {
                std::thread::sleep(RESTORED_SLOT_SETTLE);
                return Ok(());
            }
            Ok(resp) => {
                return Err(FcError::WarmReadyProbeRejected {
                    status: resp.status,
                    exit_code: resp.exit_code,
                });
            }
            Err(e) => {
                last_error = Some(e);
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }
    Err(last_error.unwrap_or(FcError::WarmReadyProbeNoResult))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{BackendConfig, CgroupMode};

    const FC_VERSION: &str = "v1.15.1";

    #[test]
    fn warm_snapshot_verify_accepts_matching_manifest() {
        let dir = tempfile::tempdir().expect("snapshot dir");
        let paths = write_snapshot_pair(dir.path());
        m80_snapshot::write_snapshot_manifest(&paths, FC_VERSION).expect("write manifest");

        verify_warm_snapshot(&paths, FC_VERSION).expect("matching warm snapshot");
    }

    #[test]
    fn warm_snapshot_verify_rejects_tampered_memory_before_slot_launch() {
        let dir = tempfile::tempdir().expect("snapshot dir");
        let paths = write_snapshot_pair(dir.path());
        m80_snapshot::write_snapshot_manifest(&paths, FC_VERSION).expect("write manifest");
        std::fs::write(&paths.mem, b"tampered-memory").expect("tamper memory");

        let err = verify_warm_snapshot(&paths, FC_VERSION)
            .expect_err("tampered warm snapshot must be rejected");

        assert!(
            matches!(
                err,
                FcError::Snapshot(m80_snapshot::SnapshotError::ArtifactMismatch {
                    kind: m80_snapshot::ArtifactKind::Memory,
                    ..
                })
            ),
            "expected memory artifact mismatch, got {err:?}"
        );
    }

    #[test]
    fn warm_pool_fill_rejects_tampered_snapshot_before_admission() {
        let run_root = tempfile::tempdir().expect("run root");
        let snapshot_dir = run_root.path().join("warm/snapshot");
        std::fs::create_dir_all(&snapshot_dir).expect("snapshot dir");
        let paths = write_snapshot_pair(&snapshot_dir);
        let discovery = fake_discovery(run_root.path());
        let expected_firecracker_version = discovery.manifest.expected_firecracker_version.clone();
        m80_snapshot::write_snapshot_manifest(&paths, &expected_firecracker_version)
            .expect("write manifest");
        std::fs::write(&paths.mem, b"tampered-memory").expect("tamper memory");

        let backend = Arc::new(
            Backend::new(
                BackendConfig::builder(discovery)
                    .max_concurrent_vms(1)
                    .run_root(run_root.path())
                    .cgroup_mode(CgroupMode::Disabled)
                    .build(),
            )
            .expect("backend"),
        );
        let pool = WarmPool::new(
            backend,
            WarmPoolConfig {
                target_ready: 1,
                snapshot: paths,
                sandbox: SandboxConfig::default(),
                ready_probe: ExecRequest {
                    program: "/bin/true".to_owned(),
                    args: Vec::new(),
                    cwd: None,
                    env: None,
                    stdin: None,
                    timeout_ms: Some(5_000),
                    streaming: false,
                },
                vm_id_prefix: "tampered-warm".to_owned(),
                cpu_allocator: None,
            },
        )
        .expect("warm pool");

        let err = pool
            .fill_to_target_blocking()
            .expect_err("tampered snapshot must fail before launch");

        assert!(
            matches!(
                err,
                FcError::Snapshot(m80_snapshot::SnapshotError::ArtifactMismatch {
                    kind: m80_snapshot::ArtifactKind::Memory,
                    ..
                })
            ),
            "expected memory artifact mismatch, got {err:?}"
        );
        assert_eq!(pool.snapshot().ready, 0);
    }

    fn write_snapshot_pair(dir: &std::path::Path) -> SnapshotPaths {
        let paths = SnapshotPaths {
            vm_state: dir.join("vm.snap"),
            mem: dir.join("mem.snap"),
        };
        std::fs::write(&paths.vm_state, b"vm-state").expect("write vm snapshot");
        std::fs::write(&paths.mem, b"memory").expect("write memory snapshot");
        paths
    }

    fn fake_discovery(run_root: &std::path::Path) -> m80_preflight::Discovery {
        let rootfs = tempfile::NamedTempFile::new().expect("fake rootfs");
        let rootfs_path = rootfs.path().to_path_buf();
        let rootfs_file = rootfs.reopen().expect("fake rootfs fd");
        m80_preflight::Discovery {
            firecracker_bin: "/tmp/firecracker".into(),
            firecracker_seccomp_filter: "/tmp/firecracker-seccomp-filter.json".into(),
            jailer_bin: "/tmp/jailer".into(),
            jailer_harden_bin: "/tmp/m80-jailer-harden".into(),
            kernel: "/tmp/vmlinux".into(),
            rootfs: "/tmp/rootfs.ext4".into(),
            pinned_rootfs: m80_preflight::PinnedRootfs::from_file(rootfs_path, rootfs_file),
            manifest: m80_image_manifest::Manifest::new(
                "/tmp/m80-guestd".into(),
                "0".repeat(64),
                "v1.0.0".to_owned(),
                52,
                m80_image_manifest::ImageKind::Minimal,
                "/tmp/vmlinux".into(),
                "1".repeat(64),
                m80_image_manifest::KernelKind::Stock,
                None,
                "/tmp/rootfs.ext4".into(),
                "2".repeat(64),
                "M80_READY".to_owned(),
                m80_image_manifest::RootfsFormat::Ext4,
                None,
                None,
            ),
            run_root: run_root.to_path_buf(),
            privilege: m80_preflight::PrivilegeStatus::Root,
            report: Vec::new(),
        }
    }
}
