//! VM-lifecycle event log, per-VM probe, health rollup, Prometheus
//! rendering. See `README.md` for the crate contract.
//! Behavior captures: bead epic `m80-1f8`.

#![deny(missing_docs)]

mod diagnostics;
mod health;
mod probe;
mod prometheus;

use std::io;

pub use diagnostics::{
    Diagnostics, EventKind, ExitReason, Phase, PhaseOutcome, SourceClass, VmEvent,
    DIAGNOSTICS_FILE_NAME, DIAGNOSTICS_SCHEMA_VERSION,
};
pub use health::{aggregate_health, render_health_json, HealthSnapshot, OpsMetrics};
pub use probe::{probe, VmHealth, VmProbeRecord};
pub use prometheus::render_prometheus;

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
