//! VM-lifecycle event log, per-VM probe, health rollup, Prometheus
//! rendering. See `README.md` for the crate contract.
//! Behavior captures: bead epic `m80-1f8`.

#![deny(missing_docs)]

mod diagnostics;
#[cfg(any(feature = "_test_internal", test))]
mod health;
#[cfg(any(feature = "_test_internal", test))]
mod probe;
#[cfg(any(feature = "_test_internal", test))]
mod prometheus;

use std::io;

pub use diagnostics::{
    Diagnostics, EventKind, ExitReason, Phase, PhaseOutcome, SourceClass, VmEvent,
    DIAGNOSTICS_FILE_NAME, DIAGNOSTICS_SCHEMA_VERSION,
};
/// Probe, health-rollup, and Prometheus-rendering symbols.  No production
/// consumer exists in this workspace; they are gated behind `_test_internal`
/// so integration tests can import them without widening the default public
/// surface.
#[cfg(feature = "_test_internal")]
pub use health::{aggregate_health, render_health_json, HealthSnapshot, OpsMetrics};
#[cfg(feature = "_test_internal")]
pub use probe::{probe, VmHealth, VmProbeRecord};
#[cfg(feature = "_test_internal")]
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
