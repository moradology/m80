//! Warm-pool allocator built on top of snapshot restore.

use std::collections::VecDeque;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use m80_proto::{ExecExit, ExecRequest, ExecResponse, ExecStatus};
use m80_snapshot::SnapshotPaths;

use crate::error::{ConfigError, FcError};
use crate::types::{Backend, ExecChunk, RunningSandbox, SandboxConfig};

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

/// Warm-pool configuration.
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

/// Snapshot-backed warm pool.
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
    ready: VecDeque<RunningSandbox>,
    filling: usize,
    leased: usize,
    discarded: usize,
    last_fill_error: Option<String>,
    /// Number of consecutive slot-launch failures; reset to 0 on success.
    /// Used to index into `FILL_BACKOFF` to throttle retry threads.
    consecutive_fill_errors: u32,
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
        Ok(WarmPool {
            inner: Arc::new(WarmPoolInner {
                backend,
                config,
                state: Mutex::new(WarmPoolState {
                    ready: VecDeque::new(),
                    filling: 0,
                    leased: 0,
                    discarded: 0,
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
            let slot = self.inner.launch_slot()?;
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
        Ok(WarmLease {
            sandbox: Some(sandbox),
            pool: Arc::clone(&self.inner),
        })
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
                    return Err(FcError::Config(ConfigError::Other(format!(
                        "warm pool fill failed: {err}"
                    ))));
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

    /// Return an observable state snapshot.
    pub fn snapshot(&self) -> WarmPoolSnapshot {
        self.inner.snapshot()
    }
}

impl Drop for WarmPool {
    fn drop(&mut self) {
        self.inner.shutdown.store(true, Ordering::Relaxed);
        let mut state = self.inner.state.lock().unwrap_or_else(|p| p.into_inner());
        let ready: Vec<RunningSandbox> = state.ready.drain(..).collect();
        self.inner.changed.notify_all();
        drop(state);
        for sandbox in ready {
            let _ = discard_sandbox(sandbox);
        }
    }
}

impl WarmPoolInner {
    fn launch_slot(&self) -> Result<RunningSandbox, FcError> {
        let slot_id = self.next_slot.fetch_add(1, Ordering::Relaxed);
        let mut sandbox_config = self.config.sandbox.clone();
        sandbox_config.vm_id = Some(format!("{}-{slot_id}", self.config.vm_id_prefix));
        let sandbox = self.backend.admit(sandbox_config)?;
        let mut running = sandbox
            .launch_from_snapshot(self.config.snapshot.clone(), &self.backend.config.discovery)?;
        run_ready_probe(&mut running, &self.config.ready_probe)?;
        Ok(running)
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
                state.filling += 1;
            }

            let inner = Arc::clone(self);
            std::thread::spawn(move || {
                let launched = inner.launch_slot();
                let backoff = {
                    let mut state = inner.state.lock().unwrap_or_else(|p| p.into_inner());
                    state.filling = state.filling.saturating_sub(1);
                    match launched {
                        Ok(sandbox) if inner.shutdown.load(Ordering::Relaxed) => {
                            drop(state);
                            let _ = discard_sandbox(sandbox);
                            inner.changed.notify_all();
                            return;
                        }
                        Ok(sandbox) => {
                            state.ready.push_back(sandbox);
                            state.last_fill_error = None;
                            state.consecutive_fill_errors = 0;
                            inner.changed.notify_all();
                            None
                        }
                        Err(e) => {
                            state.consecutive_fill_errors =
                                state.consecutive_fill_errors.saturating_add(1);
                            let idx = (state.consecutive_fill_errors as usize - 1)
                                .min(FILL_BACKOFF.len() - 1);
                            let delay = FILL_BACKOFF[idx];
                            state.last_fill_error = Some(e.to_string());
                            inner.changed.notify_all();
                            Some(delay)
                        }
                    }
                };
                if let Some(delay) = backoff {
                    std::thread::sleep(delay);
                    inner.start_background_fill();
                }
            });
        }
    }

    fn lease_finished(&self) {
        {
            let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
            state.leased = state.leased.saturating_sub(1);
            state.discarded += 1;
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

/// A leased warm-pool slot.
pub struct WarmLease {
    sandbox: Option<RunningSandbox>,
    pool: Arc<WarmPoolInner>,
}

impl WarmLease {
    /// Run one exec request on the leased slot.
    pub fn exec(&mut self, req: ExecRequest) -> Result<ExecResponse, FcError> {
        self.sandbox
            .as_mut()
            .expect("warm lease holds sandbox until discard")
            .exec(req)
    }

    /// Run one exec request on the leased slot with a caller-supplied opaque
    /// request id for wire frames and diagnostics.
    pub fn exec_with_request_id(
        &mut self,
        req: ExecRequest,
        request_id: impl Into<String>,
    ) -> Result<ExecResponse, FcError> {
        let sandbox = self
            .sandbox
            .as_mut()
            .expect("warm lease holds sandbox until discard");
        with_sandbox_request_id(sandbox, request_id.into(), |sandbox| sandbox.exec(req))
    }

    /// Run one exec request on the leased slot and forward stdout/stderr
    /// chunks as guestd emits them.
    pub fn exec_streaming(
        &mut self,
        req: ExecRequest,
        on_chunk: impl FnMut(ExecChunk) -> Result<(), FcError>,
    ) -> Result<ExecExit, FcError> {
        self.sandbox
            .as_mut()
            .expect("warm lease holds sandbox until discard")
            .exec_streaming(req, on_chunk)
    }

    /// Run one streaming exec request on the leased slot with a caller-supplied
    /// opaque request id for wire frames and diagnostics.
    pub fn exec_streaming_with_request_id(
        &mut self,
        req: ExecRequest,
        request_id: impl Into<String>,
        on_chunk: impl FnMut(ExecChunk) -> Result<(), FcError>,
    ) -> Result<ExecExit, FcError> {
        let sandbox = self
            .sandbox
            .as_mut()
            .expect("warm lease holds sandbox until discard");
        with_sandbox_request_id(sandbox, request_id.into(), |sandbox| {
            sandbox.exec_streaming(req, on_chunk)
        })
    }

    /// VM id for the leased slot.
    pub fn vm_id(&self) -> &str {
        self.sandbox
            .as_ref()
            .expect("warm lease holds sandbox until discard")
            .vm_id()
    }

    /// Run directory for the leased slot. Useful for diagnostics before a
    /// failed lease is discarded.
    pub fn run_dir(&self) -> &Path {
        self.sandbox
            .as_ref()
            .expect("warm lease holds sandbox until discard")
            .run_dir()
    }

    /// Consume the lease, force-kill the slot, delete its run-dir, and allow
    /// the pool to refill a replacement.
    pub fn discard(mut self) -> Result<(), FcError> {
        let sandbox = self.sandbox.take();
        let result = if let Some(sandbox) = sandbox {
            discard_sandbox(sandbox)
        } else {
            Ok(())
        };
        self.pool.lease_finished();
        self.pool.start_background_fill();
        result
    }
}

impl Drop for WarmLease {
    fn drop(&mut self) {
        if let Some(sandbox) = self.sandbox.take() {
            let _ = discard_sandbox(sandbox);
            self.pool.lease_finished();
            self.pool.start_background_fill();
        }
    }
}

fn discard_sandbox(sandbox: RunningSandbox) -> Result<(), FcError> {
    sandbox.force_kill()?.delete()
}

fn with_sandbox_request_id<T>(
    sandbox: &mut RunningSandbox,
    request_id: String,
    f: impl FnOnce(&mut RunningSandbox) -> Result<T, FcError>,
) -> Result<T, FcError> {
    let old = sandbox.request_id.replace(request_id);
    let result = f(sandbox);
    sandbox.request_id = old;
    result
}

fn run_ready_probe(sandbox: &mut RunningSandbox, req: &ExecRequest) -> Result<(), FcError> {
    let mut last_error = None;
    for _ in 0..5 {
        match sandbox.exec(req.clone()) {
            Ok(resp) if resp.status == ExecStatus::Completed && resp.exit_code == Some(0) => {
                std::thread::sleep(RESTORED_SLOT_SETTLE);
                return Ok(());
            }
            Ok(resp) => {
                return Err(FcError::Config(ConfigError::Other(format!(
                    "warm pool ready probe failed: status={:?} exit_code={:?}",
                    resp.status, resp.exit_code
                ))));
            }
            Err(e) => {
                last_error = Some(e);
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }
    Err(last_error.unwrap_or_else(|| {
        FcError::Config(ConfigError::Other("warm pool ready probe failed".into()))
    }))
}
