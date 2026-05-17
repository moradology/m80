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
/// Canonical tracing span names and field schemas for VM-mechanics surfaces.
pub mod spans;

use std::io;
use std::path::PathBuf;

pub use diagnostics::{
    Diagnostics, ExitReason, Phase, PhaseOutcome, VmEvent, DIAGNOSTICS_FILE_NAME,
};
/// Probe, health-rollup, and Prometheus-rendering symbols.  No production
/// consumer exists in this workspace; they are gated behind `_test_internal`
/// so integration tests can import them without widening the default public
/// surface.
#[cfg(feature = "_test_internal")]
pub use health::{
    aggregate_health, render_health_json, DurationHistogram, HealthSnapshot, LeaseAttribution,
    MetricLabelError, MetricLabelValue, OpsMetrics, PmemLayerCountBySharing, PmemSharingLabel,
    PostRestoreHookDuration, PostRestoreHookVariantLabel, ScratchSourceLabel,
    TemplateCountByFreshness, TemplateFreshnessLabel,
};
#[cfg(feature = "_test_internal")]
pub use probe::{probe, VmHealth, VmProbeRecord};
#[cfg(feature = "_test_internal")]
pub use prometheus::render_prometheus;

/// Errors surfaced by observability operations.
///
/// Public because `Diagnostics::open`, `Diagnostics::record`, and the
/// `_test_internal` probe/health helpers return it directly.
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
