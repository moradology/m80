//! VM-lifecycle event log, per-VM probe, health rollup, Prometheus
//! rendering. Execution lane deferred to v0.2; v0.1 ships
//! [`Diagnostics::disabled`] + types so callers compile against the
//! stable surface today. See `README.md` for the contract.
//! Behavior captures: bead epic `m80-1f8`.

#![deny(missing_docs)]

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// A handle to per-VM diagnostics. Created by [`Diagnostics::disabled`] in
/// v0.1; v0.2 adds an enabled constructor that opens the `diagnostics.jsonl`.
#[derive(Debug, Default)]
pub struct Diagnostics;

impl Diagnostics {
    /// The v0.1 entrypoint: returns a no-op [`Diagnostics`] handle.
    pub fn disabled() -> Self {
        Self
    }

    /// Append one [`VmEvent`] to the diagnostics log. v0.1 no-op.
    pub fn record(&mut self, _event: &VmEvent) -> Result<(), ObservabilityError> {
        Ok(())
    }
}

/// One structured event for the diagnostics log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VmEvent {
    /// Caller-supplied free-text detail.
    pub detail: String,
    /// Lifecycle phase the event belongs to.
    pub phase: Phase,
    /// Unix epoch milliseconds at the time of recording.
    pub timestamp_unix_ms: u64,
}

/// VM lifecycle phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    /// `m80-firecracker-client` PUTs + `InstanceStart`.
    Boot,
    /// Run-dir teardown.
    Delete,
    /// `m80-preflight` stage.
    HostPreflight,
    /// Vsock ready-marker observed.
    Ready,
    /// Graceful or forced stop.
    Stop,
    /// `m80-storage` rootfs clone + scratch hydration.
    StoragePrepare,
    /// Run-root recovery loop reaped a stale run-dir.
    StartupScavenge,
}

/// One per-VM probe record. v0.2 surface; v0.1 stub.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmProbeRecord {
    /// Health classification.
    pub health: VmHealth,
    /// Path of the run-dir.
    pub run_dir: PathBuf,
    /// VM identifier.
    pub vm_id: String,
}

/// Health classification produced by the probe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VmHealth {
    /// Live but with degraded indicators.
    Degraded,
    /// Process tree gone; residue remains.
    Exited,
    /// Sockets responsive, ownership markers consistent.
    Healthy,
    /// Live but unresponsive.
    Stuck,
}

/// Probe over a run-root and emit one record per VM.
///
/// **Returns [`ObservabilityError::Deferred`] in v0.1.**
pub fn probe(_run_root: &Path) -> Result<Vec<VmProbeRecord>, ObservabilityError> {
    Err(ObservabilityError::Deferred)
}

/// Aggregated rollup of probe records. v0.2 surface; v0.1 stub.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HealthSnapshot {
    /// Number of degraded VMs.
    pub degraded: u32,
    /// Number of exited VMs awaiting cleanup.
    pub exited: u32,
    /// Number of healthy VMs.
    pub healthy: u32,
    /// Number of stuck VMs.
    pub stuck: u32,
}

/// Per-VM operational metrics rolled up across the run-root. v0.2 surface.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OpsMetrics {
    /// Number of VMs the metrics span.
    pub vm_count: u32,
}

/// Aggregate probe records into a [`HealthSnapshot`].
///
/// **Returns [`ObservabilityError::Deferred`] in v0.1.**
pub fn aggregate_health(_records: &[VmProbeRecord]) -> Result<HealthSnapshot, ObservabilityError> {
    Err(ObservabilityError::Deferred)
}

/// Render a Prometheus exposition-format text response.
///
/// **Returns [`ObservabilityError::Deferred`] in v0.1.**
pub fn render_prometheus(
    _health: &HealthSnapshot,
    _metrics: &OpsMetrics,
) -> Result<String, ObservabilityError> {
    Err(ObservabilityError::Deferred)
}

/// Render a JSON health snapshot.
///
/// **Returns [`ObservabilityError::Deferred`] in v0.1.**
pub fn render_health_json(_health: &HealthSnapshot) -> Result<String, ObservabilityError> {
    Err(ObservabilityError::Deferred)
}

/// Errors surfaced by observability operations.
#[derive(Debug, thiserror::Error)]
pub enum ObservabilityError {
    /// Observability execution lane is deferred to v0.2.
    #[error("observability execution is deferred to v0.2")]
    Deferred,
    /// Underlying I/O failure.
    #[error("i/o: {0}")]
    Io(#[from] io::Error),
    /// JSON encode/decode failure.
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}
