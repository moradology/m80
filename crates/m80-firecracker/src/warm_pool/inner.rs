//! Warm-pool inner state operations.

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use m80_proto::{ExecRequest, ExecStatus};
use m80_snapshot::{verify_snapshot_manifest, SnapshotPaths};

use super::fill_worker::spawn_fill_worker;
use super::template_build::{build_template, template_inputs_for_current_host};
use super::{
    discard_sandbox, FcError, RunningSandbox, WarmPoolInner, WarmPoolSnapshot, WarmPoolState,
    WarmSlot, WarmStrategy, MAX_FILL_DURATION_SAMPLES, MAX_FILL_THREADS,
};

impl WarmPoolInner {
    pub(super) fn launch_slot(&self, cpuset_cpus: Option<String>) -> Result<WarmSlot, FcError> {
        #[cfg(test)]
        if self.panic_next_launch_slot.swap(false, Ordering::SeqCst) {
            panic!("injected warm-pool launch_slot panic");
        }

        let slot_id = self.next_slot.fetch_add(1, Ordering::Relaxed);
        let mut sandbox_config = self.config.sandbox.clone();
        sandbox_config.vm_id = Some(format!("{}-{slot_id}", self.config.vm_id_prefix));
        sandbox_config.cpuset_cpus = cpuset_cpus.clone();
        let running = match &self.config.strategy {
            WarmStrategy::DirectSnapshot {
                snapshot,
                ready_probe,
            } => {
                verify_warm_snapshot(
                    snapshot,
                    &self
                        .backend
                        .config
                        .discovery
                        .manifest
                        .expected_firecracker_version,
                )?;
                let sandbox = self.backend.admit(sandbox_config)?;
                let mut running = sandbox
                    .launch_from_snapshot(snapshot.clone(), &self.backend.config.discovery)?;
                run_ready_probe(&mut running, ready_probe)?;
                running
            }
            WarmStrategy::SnapshotRestore {
                store,
                hooks,
                ready_probe,
            } => {
                let inputs = template_inputs_for_current_host(
                    &self.backend,
                    &sandbox_config,
                    hooks.clone(),
                )?;
                let pinned = build_template(&self.backend, inputs, store, &sandbox_config)?;
                let sandbox = self.backend.admit(sandbox_config)?;
                let mut running = sandbox.launch_from_template_body_with_hooks(
                    &pinned,
                    &self.backend.config.discovery,
                    hooks.clone(),
                )?;
                run_ready_probe(&mut running, ready_probe)?;
                running
            }
        };
        Ok(WarmSlot {
            sandbox: running,
            cpuset_cpus,
        })
    }

    pub(super) fn start_background_fill(self: &Arc<Self>) {
        loop {
            {
                let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
                if self.shutdown.load(Ordering::Relaxed) {
                    return;
                }
                // Stop if the pool is already at target or already has the
                // maximum number of concurrent fill workers running.
                let deficit = self
                    .target_ready()
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

    pub(super) fn lease_finished(&self, cpuset_cpus: Option<String>) {
        {
            let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
            state.leased = state.leased.saturating_sub(1);
            state.discarded += 1;
            state.lease_returned_total = state.lease_returned_total.saturating_add(1);
            state.release_cpuset_cpus(cpuset_cpus);
            self.changed.notify_all();
        }
    }

    pub(super) fn target_ready(&self) -> usize {
        self.target_ready.load(Ordering::Relaxed)
    }

    pub(super) fn discard_unleased_slot(&self, slot: WarmSlot, reason: &'static str) {
        let WarmSlot {
            sandbox,
            cpuset_cpus,
        } = slot;
        let result = discard_sandbox(sandbox);
        {
            let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
            state.discarded = state.discarded.saturating_add(1);
            state.release_cpuset_cpus(cpuset_cpus);
            self.changed.notify_all();
        }
        if let Err(err) = result {
            tracing::error!(error = %err, reason, "failed to discard unleased warm-pool slot");
        }
    }

    pub(super) fn snapshot(&self) -> WarmPoolSnapshot {
        let state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        WarmPoolSnapshot {
            target_ready: self.target_ready(),
            ready: state.ready.len(),
            filling: state.filling,
            leased: state.leased,
            discarded: state.discarded,
            consecutive_fill_errors: state.consecutive_fill_errors,
            fill_attempts_total: state.fill_attempts_total,
            fill_failures_total: state.fill_failures_total,
            lease_acquired_total: state.lease_acquired_total,
            lease_returned_total: state.lease_returned_total,
        }
    }

    pub(super) fn take_fill_duration_samples_us(&self) -> Vec<u64> {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        state.take_fill_duration_samples_us()
    }
}

impl WarmSlot {
    pub(super) fn is_live(&self) -> bool {
        crate::runroot::pid_is_alive(self.sandbox.firecracker.firecracker_pid())
    }
}

impl WarmPoolState {
    pub(super) fn reserve_cpuset_cpus(&mut self) -> Option<String> {
        self.free_cpuset_cpus.pop_front()
    }

    pub(super) fn release_cpuset_cpus(&mut self, cpuset_cpus: Option<String>) {
        if let Some(cpuset_cpus) = cpuset_cpus {
            self.free_cpuset_cpus.push_back(cpuset_cpus);
        }
    }

    pub(super) fn record_fill_attempt(&mut self) {
        self.fill_attempts_total = self.fill_attempts_total.saturating_add(1);
    }

    pub(super) fn record_fill_success(&mut self, duration_us: u64) {
        self.last_fill_error = None;
        self.consecutive_fill_errors = 0;
        if self.fill_duration_samples_us.len() == MAX_FILL_DURATION_SAMPLES {
            self.fill_duration_samples_us.pop_front();
        }
        self.fill_duration_samples_us.push_back(duration_us);
    }

    pub(super) fn record_fill_failure(&mut self, detail: String) {
        self.fill_failures_total = self.fill_failures_total.saturating_add(1);
        self.discarded = self.discarded.saturating_add(1);
        self.consecutive_fill_errors = self.consecutive_fill_errors.saturating_add(1);
        self.last_fill_error = Some(detail);
    }

    pub(super) fn take_fill_duration_samples_us(&mut self) -> Vec<u64> {
        self.fill_duration_samples_us.drain(..).collect()
    }
}

pub(super) fn verify_warm_snapshot(
    paths: &SnapshotPaths,
    expected_firecracker_version: &str,
) -> Result<(), FcError> {
    verify_snapshot_manifest(paths, expected_firecracker_version)
        .map(|_| ())
        .map_err(FcError::Snapshot)
}

fn run_ready_probe(sandbox: &mut RunningSandbox, req: &ExecRequest) -> Result<(), FcError> {
    let mut last_error = None;
    for _ in 0..5 {
        match sandbox.exec_ready_probe(req.clone()) {
            Ok(resp) if resp.status == ExecStatus::Completed && resp.exit_code == Some(0) => {
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
