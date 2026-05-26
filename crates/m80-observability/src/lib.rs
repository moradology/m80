//! VM-lifecycle event log, per-VM probe, health rollup, Prometheus
//! rendering. See `README.md` for the crate contract.
//! Behavior captures: bead epic `m80-1f8`.

#![deny(missing_docs)]

mod diagnostics;
mod health;
mod probe;
mod prometheus;
/// Canonical tracing span names and field schemas for VM-mechanics surfaces.
pub mod spans;

use std::io;
use std::path::PathBuf;

pub use diagnostics::{
    Diagnostics, ExitReason, Phase, PhaseOutcome, VmEvent, DIAGNOSTICS_FILE_NAME,
};
/// Probe, health-rollup, operational metric DTOs, and Prometheus-rendering
/// symbols for production scrape integration.
pub use health::{
    aggregate_health, render_health_json, DurationHistogram, ErrorCount, HealthSnapshot,
    LeaseAttribution, MetricLabelError, MetricLabelValue, OpsMetrics, PhaseFailureCount,
    PmemLayerCountBySharing, PmemSharingLabel, PostRestoreHookDuration,
    PostRestoreHookVariantLabel, ScratchSourceLabel, TemplateCountByFreshness,
    TemplateFreshnessLabel, WarmPoolMetrics,
};
pub use probe::{probe, VmHealth, VmProbeRecord};
pub use prometheus::render_prometheus;

/// Errors surfaced by observability operations.
///
/// Public because `Diagnostics::open`, `Diagnostics::record`, and the
/// probe/health helpers return it directly.
#[derive(Debug, thiserror::Error)]
pub enum ObservabilityError {
    /// Filesystem I/O failure where the target path is known.
    #[error("i/o on {}: {source}", path.display())]
    PathIo {
        /// Path the operation targeted.
        path: PathBuf,
        /// Underlying I/O error.
        #[source]
        source: io::Error,
    },
    /// JSON encode/decode failure.
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}
