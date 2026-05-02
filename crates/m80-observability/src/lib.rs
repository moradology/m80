//! VM-lifecycle event log, per-VM probe, health rollup, Prometheus rendering.
//! Deferred to v0.2.
//!
//! See `README.md` for the black-box contract.
//! Behavior captures: bead epic `m80-1f8` (`br show m80-1f8`).
//!
//! # Type-pinning pass (v0.1)
//!
//! v0.1 exposes only [`Diagnostics::disabled`] so callers can write
//! `Option<Diagnostics>` against a stable type without conditional
//! compilation. The full v0.2 surface (Probe, HealthSnapshot, OpsMetrics,
//! `render_prometheus`) is declared here as opaque types so consumers can
//! `use` them today.

#![deny(missing_docs)]

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// A handle to per-VM diagnostics. Created by [`Diagnostics::disabled`] in
/// v0.1; v0.2 adds an enabled constructor that opens the `diagnostics.jsonl`.
#[derive(Debug)]
pub struct Diagnostics {
    _enabled: bool,
}

impl Diagnostics {
    /// The v0.1 entrypoint: returns a no-op [`Diagnostics`] handle.
    pub fn disabled() -> Self {
        Self { _enabled: false }
    }

    /// Append one [`VmEvent`] to the diagnostics log. v0.1 no-op.
    pub fn record(&mut self, _event: &VmEvent) -> Result<(), ObservabilityError> {
        Ok(())
    }
}

/// One structured event for the diagnostics log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmEvent {
    /// Lifecycle phase the event belongs to.
    pub phase: Phase,
    /// Caller-supplied free-text detail.
    pub detail: String,
    /// Unix epoch milliseconds at the time of recording.
    pub timestamp_unix_ms: u64,
}

/// VM lifecycle phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    /// Run-root recovery loop reaped a stale run-dir.
    StartupScavenge,
    /// `m80-preflight` stage.
    HostPreflight,
    /// `m80-storage` rootfs clone + scratch hydration.
    StoragePrepare,
    /// `m80-firecracker-client` PUTs + `InstanceStart`.
    Boot,
    /// Vsock ready-marker observed.
    Ready,
    /// Graceful or forced stop.
    Stop,
    /// Run-dir teardown.
    Delete,
}

/// One per-VM probe record. v0.2 surface; v0.1 stub.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmProbeRecord {
    /// VM identifier.
    pub vm_id: String,
    /// Path of the run-dir.
    pub run_dir: PathBuf,
    /// Health classification.
    pub health: VmHealth,
}

/// Health classification produced by the probe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VmHealth {
    /// Sockets responsive, ownership markers consistent.
    Healthy,
    /// Live but with degraded indicators.
    Degraded,
    /// Live but unresponsive.
    Stuck,
    /// Process tree gone; residue remains.
    Exited,
}

/// Probe over a run-root and emit one record per VM. **v0.2 — todo!()**.
pub fn probe(_run_root: &Path) -> Result<Vec<VmProbeRecord>, ObservabilityError> {
    todo!("v0.2")
}

/// Aggregated rollup of probe records. v0.2 surface; v0.1 stub.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HealthSnapshot {
    /// Number of healthy VMs.
    pub healthy: u32,
    /// Number of degraded VMs.
    pub degraded: u32,
    /// Number of stuck VMs.
    pub stuck: u32,
    /// Number of exited VMs awaiting cleanup.
    pub exited: u32,
}

/// Per-VM operational metrics rolled up across the run-root. v0.2 surface.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OpsMetrics {
    /// Number of VMs the metrics span.
    pub vm_count: u32,
}

/// Aggregate probe records into a [`HealthSnapshot`]. **v0.2 — todo!()**.
pub fn aggregate_health(_records: &[VmProbeRecord]) -> HealthSnapshot {
    todo!("v0.2")
}

/// Render a Prometheus exposition-format text response. **v0.2 — todo!()**.
pub fn render_prometheus(_health: &HealthSnapshot, _metrics: &OpsMetrics) -> String {
    todo!("v0.2")
}

/// Render a JSON health snapshot. **v0.2 — todo!()**.
pub fn render_health_json(_health: &HealthSnapshot) -> String {
    todo!("v0.2")
}

/// Errors surfaced by observability operations.
#[derive(Debug, thiserror::Error)]
pub enum ObservabilityError {
    /// Underlying I/O failure.
    #[error("i/o: {0}")]
    Io(#[from] io::Error),
    /// JSON encode/decode failure.
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}
