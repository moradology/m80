//! Warm-pool allocator built on top of snapshot restore.

mod cpu_allocator;
mod fill_worker;
mod inner;
mod lease;
mod template;
mod template_build;

use std::collections::VecDeque;
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use crate::error::{ConfigError, FcError};
use crate::types::{Backend, RunningSandbox, SandboxConfig};

use cpu_allocator::build_warm_pool_cpu_ranges;
pub use cpu_allocator::WarmPoolCpuAllocator;
#[cfg(test)]
use inner::verify_warm_snapshot;
pub use lease::WarmLease;
pub use template::*;
pub(crate) use template_build::{build_template, template_inputs_for_current_host};

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

/// Number of successful fill-duration samples retained for measurement reads.
const MAX_FILL_DURATION_SAMPLES: usize = 1024;

/// Parameters that govern a [`WarmPool`]. All fields are required at
/// construction; `target_ready` must be greater than zero and
/// `sandbox.workspace` must be `None` — warm slots are stateless and may not
/// carry a workspace.
#[derive(Debug, Clone)]
pub struct WarmPoolConfig {
    /// Number of ready slots the pool tries to keep filled.
    pub target_ready: usize,
    /// Sandbox configuration applied to each restored slot.
    pub sandbox: SandboxConfig,
    /// Strategy used to fill ready slots.
    pub strategy: WarmStrategy,
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
    /// Consecutive slot-launch failures since the last successful fill.
    pub consecutive_fill_errors: u32,
    /// Slot-fill attempts since pool creation.
    pub fill_attempts_total: usize,
    /// Slot-fill failures since pool creation.
    pub fill_failures_total: usize,
    /// Leases handed to callers since pool creation.
    pub lease_acquired_total: usize,
    /// Leases released back to the pool since pool creation.
    pub lease_returned_total: usize,
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
    target_ready: AtomicUsize,
    cpu_allocator_capacity: Option<usize>,
    state: Mutex<WarmPoolState>,
    changed: Condvar,
    shutdown: AtomicBool,
    next_slot: AtomicU64,
    #[cfg(test)]
    panic_next_launch_slot: AtomicBool,
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
    fill_attempts_total: usize,
    fill_failures_total: usize,
    lease_acquired_total: usize,
    lease_returned_total: usize,
    fill_duration_samples_us: VecDeque<u64>,
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
        validate_target_ready_against_backend(&backend, config.target_ready)?;
        if config.sandbox.workspace.is_some() {
            return Err(FcError::Config(ConfigError::InvalidValue {
                field: "warm_pool.sandbox.workspace",
                reason: "warm pool slots must be stateless: SandboxConfig::workspace must be None"
                    .into(),
            }));
        }
        let cpu_allocator_capacity = config.cpu_allocator.as_ref().map(|_| config.target_ready);
        let free_cpuset_cpus =
            build_warm_pool_cpu_ranges(config.target_ready, config.cpu_allocator)?
                .into_iter()
                .collect();
        Ok(WarmPool {
            inner: Arc::new(WarmPoolInner {
                backend,
                target_ready: AtomicUsize::new(config.target_ready),
                cpu_allocator_capacity,
                config,
                state: Mutex::new(WarmPoolState {
                    ready: VecDeque::new(),
                    filling: 0,
                    leased: 0,
                    discarded: 0,
                    free_cpuset_cpus,
                    last_fill_error: None,
                    consecutive_fill_errors: 0,
                    fill_attempts_total: 0,
                    fill_failures_total: 0,
                    lease_acquired_total: 0,
                    lease_returned_total: 0,
                    fill_duration_samples_us: VecDeque::new(),
                }),
                changed: Condvar::new(),
                shutdown: AtomicBool::new(false),
                next_slot: AtomicU64::new(0),
                #[cfg(test)]
                panic_next_launch_slot: AtomicBool::new(false),
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
                if state.ready.len() >= self.inner.target_ready() {
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
            {
                let mut state = self.inner.state.lock().unwrap_or_else(|p| p.into_inner());
                state.record_fill_attempt();
            }
            let fill_started = Instant::now();
            let slot = match self.inner.launch_slot(cpuset_cpus.clone()) {
                Ok(slot) => slot,
                Err(err) => {
                    let mut state = self.inner.state.lock().unwrap_or_else(|p| p.into_inner());
                    state.release_cpuset_cpus(cpuset_cpus);
                    state.record_fill_failure(err.to_string());
                    self.inner.changed.notify_all();
                    return Err(err);
                }
            };
            let fill_duration_us = duration_micros_u64(fill_started.elapsed());
            let mut state = self.inner.state.lock().unwrap_or_else(|p| p.into_inner());
            state.ready.push_back(slot);
            state.record_fill_success(fill_duration_us);
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
        let mut discarded_dead_slot = false;
        loop {
            let slot = {
                let mut state = self.inner.state.lock().unwrap_or_else(|p| p.into_inner());
                match state.ready.pop_front() {
                    Some(slot) => slot,
                    None => {
                        if discarded_dead_slot {
                            drop(state);
                            self.inner.start_background_fill();
                        }
                        return Err(FcError::PoolEmpty {
                            target_ready: self.inner.target_ready(),
                        });
                    }
                }
            };
            if slot.is_live() {
                {
                    let mut state = self.inner.state.lock().unwrap_or_else(|p| p.into_inner());
                    state.leased += 1;
                    state.lease_acquired_total = state.lease_acquired_total.saturating_add(1);
                    self.inner.changed.notify_all();
                }
                self.inner.start_background_fill();
                return Ok(WarmLease::new(slot, Arc::clone(&self.inner)));
            }
            discarded_dead_slot = true;
            self.inner.discard_unleased_slot(slot, "dead ready slot");
        }
    }

    /// Adjust the ready-slot target at runtime.
    ///
    /// Growing starts normal background fill. Shrinking discards only surplus
    /// ready slots; leased slots are never force-dropped by resize.
    pub fn set_target_ready(&self, target_ready: NonZeroUsize) -> Result<NonZeroUsize, FcError> {
        let target_ready = target_ready.get();
        validate_target_ready_against_backend(&self.inner.backend, target_ready)?;
        if let Some(capacity) = self.inner.cpu_allocator_capacity {
            if target_ready > capacity {
                return Err(FcError::Config(ConfigError::InvalidValue {
                    field: "warm_pool.target_ready",
                    reason: format!("must be <= initial cpuset allocator capacity ({capacity})"),
                }));
            }
        }

        let discard = {
            let mut state = self.inner.state.lock().unwrap_or_else(|p| p.into_inner());
            self.inner
                .target_ready
                .store(target_ready, Ordering::Relaxed);
            let mut discard = Vec::new();
            while state.ready.len() > target_ready {
                let slot = state
                    .ready
                    .pop_back()
                    .expect("ready length checked before pop");
                let WarmSlot {
                    sandbox,
                    cpuset_cpus,
                } = slot;
                state.discarded = state.discarded.saturating_add(1);
                discard.push((sandbox, cpuset_cpus));
            }
            self.inner.changed.notify_all();
            discard
        };
        for (sandbox, cpuset_cpus) in discard {
            if let Err(err) = discard_sandbox(sandbox) {
                tracing::error!(error = %err, "failed to discard surplus warm-pool slot after resize");
            }
            let mut state = self.inner.state.lock().unwrap_or_else(|p| p.into_inner());
            state.release_cpuset_cpus(cpuset_cpus);
            self.inner.changed.notify_all();
        }
        self.inner.start_background_fill();
        Ok(NonZeroUsize::new(target_ready).expect("input was NonZeroUsize"))
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
                    target_ready: self.inner.target_ready(),
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

    /// Drain successful fill-duration samples recorded since pool creation or
    /// the previous call. Each sample starts immediately before slot launch
    /// work and stops when the slot is accepted into the ready queue.
    ///
    /// For [`WarmStrategy::SnapshotRestore`], this includes template
    /// lookup/build, snapshot restore, post-restore hooks, and ready probes. It
    /// does not include the caller's subsequent [`WarmPool::try_lease`] handoff;
    /// measurement harnesses that need restore-to-handback latency should add
    /// their checkout timing to the drained fill sample.
    #[must_use]
    pub fn take_fill_duration_samples_us(&self) -> Vec<u64> {
        self.inner.take_fill_duration_samples_us()
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
                let discarded = state.ready.len();
                state.discarded = state.discarded.saturating_add(discarded);
                state.ready.drain(..).collect::<Vec<_>>()
            };
            for slot in ready {
                let WarmSlot { sandbox, .. } = slot;
                if let Err(err) = discard_sandbox(sandbox) {
                    tracing::error!(error = %err, "failed to discard warm-pool slot during drop");
                }
            }
        }
    }
}

fn validate_target_ready_against_backend(
    backend: &Arc<Backend>,
    target_ready: usize,
) -> Result<(), FcError> {
    let max = backend.config().max_concurrent_vms() as usize;
    if target_ready <= max {
        return Ok(());
    }
    Err(FcError::Config(ConfigError::InvalidValue {
        field: "warm_pool.target_ready",
        reason: format!("must be <= backend.max_concurrent_vms ({max})"),
    }))
}

fn discard_sandbox(sandbox: RunningSandbox) -> Result<(), FcError> {
    sandbox.force_kill()?.delete()
}

fn duration_micros_u64(duration: Duration) -> u64 {
    u64::try_from(duration.as_micros()).unwrap_or(u64::MAX)
}

/// Discard a sandbox, first recording a diagnostics event if `exec_err` is
/// `Some` (i.e., we are discarding after an exec failure, not normal teardown).
pub(super) fn discard_sandbox_with_diagnostics(
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

#[cfg(test)]
mod tests;
