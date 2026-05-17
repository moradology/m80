use serde::{Deserialize, Serialize};

use m80_proto::MetricsResponse;

use crate::probe::{VmHealth, VmProbeRecord};

const MAX_METRIC_LABEL_VALUE_LEN: usize = 256;

/// Aggregated rollup of probe records.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HealthSnapshot {
    /// Number of degraded VMs.
    pub degraded: u32,
    /// Number of exited VMs awaiting cleanup.
    pub exited: u32,
    /// Number of healthy VMs.
    pub healthy: u32,
    /// Number of stuck VMs.
    pub stuck: u32,
    /// Total records included in this snapshot.
    pub total: u32,
    /// Whether all visible VMs are healthy.
    pub rollout_ready: bool,
}

/// Per-VM operational metrics rolled up across the run-root.
#[derive(Debug, Clone, Default)]
pub struct OpsMetrics {
    /// Number of VMs the metrics span.
    pub vm_count: u32,
    /// Guest metrics sampled from one running VM at scrape time.
    pub guest: Option<MetricsResponse>,
    // TODO(m80-q420k.5.8): producer-side wiring for the layered-rootfs
    // families belongs with the real-run E2E surface; zero defaults keep the
    // render contract available until then.
    /// Pmem layer count by declared sharing mode.
    pub pmem_layers_per_vm_count_by_sharing: PmemLayerCountBySharing,
    /// Snapshot-template count by freshness classification.
    pub template_count_by_freshness: TemplateCountByFreshness,
    /// Warm restore latency observations.
    pub restore_latency_seconds: DurationHistogram,
    /// Post-restore hook duration observations grouped by hook variant.
    pub post_restore_hook_duration_seconds: Vec<PostRestoreHookDuration>,
    /// Total bytes currently occupied by the image store.
    pub image_store_bytes: u64,
    /// Total bytes currently occupied by the snapshot-template store.
    pub template_store_bytes: u64,
    /// Info-style attribution records for active leases.
    pub lease_attribution: Vec<LeaseAttribution>,
}

/// Pmem layer counts split by `PmemSharing` mode.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PmemLayerCountBySharing {
    /// Count for `PmemSharing::PerVm`.
    pub per_vm: u64,
    /// Count for `PmemSharing::Shared`.
    pub shared: u64,
}

/// Template counts split by freshness classification.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TemplateCountByFreshness {
    /// Count of templates matching their current input fingerprint.
    pub fresh: u64,
    /// Count of templates invalidated by input or fingerprint drift.
    pub invalidated: u64,
}

/// Duration observations stored as microseconds for Prometheus seconds output.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DurationHistogram {
    observations_us: Vec<u64>,
}

impl DurationHistogram {
    /// Build a duration histogram from microsecond observations.
    pub fn from_micros(observations_us: impl IntoIterator<Item = u64>) -> Self {
        Self {
            observations_us: observations_us.into_iter().collect(),
        }
    }

    /// Return the raw microsecond observations.
    #[must_use]
    pub fn observations_us(&self) -> &[u64] {
        &self.observations_us
    }
}

/// Duration observations for one post-restore hook variant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostRestoreHookDuration {
    /// Closed hook variant label.
    pub hook_variant: PostRestoreHookVariantLabel,
    /// Duration observations for this hook variant.
    pub duration: DurationHistogram,
}

/// Closed labels for pmem sharing metrics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PmemSharingLabel {
    /// Per-VM copied or reflinked backing.
    PerVm,
    /// Same-trust-domain shared backing.
    Shared,
}

impl PmemSharingLabel {
    /// Return the stable Prometheus label value.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PerVm => "per_vm",
            Self::Shared => "shared",
        }
    }
}

/// Closed labels for template freshness metrics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemplateFreshnessLabel {
    /// Template still matches its current fingerprint inputs.
    Fresh,
    /// Template is retained but no longer matches current inputs.
    Invalidated,
}

impl TemplateFreshnessLabel {
    /// Return the stable Prometheus label value.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fresh => "fresh",
            Self::Invalidated => "invalidated",
        }
    }
}

/// Closed labels for post-restore hook metrics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PostRestoreHookVariantLabel {
    /// `HookSpec::ReseedSystemdRandomSeed`.
    ReseedSystemdRandomSeed,
    /// `HookSpec::RegenMachineId`.
    RegenMachineId,
    /// `HookSpec::SetHostname`.
    SetHostname,
}

impl PostRestoreHookVariantLabel {
    /// Return the stable Prometheus label value.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ReseedSystemdRandomSeed => "reseed_systemd_random_seed",
            Self::RegenMachineId => "regen_machine_id",
            Self::SetHostname => "set_hostname",
        }
    }
}

/// Closed labels for lease scratch-source attribution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScratchSourceLabel {
    /// No caller workspace scratch was attached.
    None,
    /// Lease has a workspace-backed scratch image.
    Workspace,
}

impl ScratchSourceLabel {
    /// Return the stable Prometheus label value.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Workspace => "workspace",
        }
    }
}

/// Validated Prometheus label value for digest and fingerprint labels.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct MetricLabelValue(String);

impl MetricLabelValue {
    /// Construct a bounded ASCII label value.
    ///
    /// This rejects unbounded free strings before they reach Prometheus
    /// rendering. Allowed bytes are ASCII alphanumeric plus `_`, `.`, `-`,
    /// `:`, and `,`.
    pub fn new(value: impl Into<String>) -> Result<Self, MetricLabelError> {
        let value = value.into();
        if value.is_empty() {
            return Err(MetricLabelError::Empty);
        }
        if value.len() > MAX_METRIC_LABEL_VALUE_LEN {
            return Err(MetricLabelError::TooLong {
                len: value.len(),
                max: MAX_METRIC_LABEL_VALUE_LEN,
            });
        }
        for (index, byte) in value.bytes().enumerate() {
            if !is_allowed_metric_label_byte(byte) {
                return Err(MetricLabelError::InvalidByte { index, byte });
            }
        }
        Ok(Self(value))
    }

    /// Return the validated label value.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Label-validation error for metrics carrying digest/fingerprint values.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MetricLabelError {
    /// Label value was empty.
    #[error("metric label value is empty")]
    Empty,
    /// Label value exceeded the bounded length.
    #[error("metric label value length {len} exceeds max {max}")]
    TooLong {
        /// Observed byte length.
        len: usize,
        /// Maximum byte length.
        max: usize,
    },
    /// Label value contained a rejected byte.
    #[error("metric label value byte 0x{byte:02x} at {index} is not allowed")]
    InvalidByte {
        /// Byte offset.
        index: usize,
        /// Rejected byte.
        byte: u8,
    },
}

/// Info-style attribution for one active lease.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeaseAttribution {
    /// Template fingerprint used for this lease.
    pub template_fingerprint: MetricLabelValue,
    /// Deterministic digest-set label for pmem layers used by this lease.
    pub pmem_digest_set: MetricLabelValue,
    /// Closed scratch source label.
    pub scratch_source: ScratchSourceLabel,
}

/// Aggregate probe records into a [`HealthSnapshot`].
pub fn aggregate_health(records: &[VmProbeRecord]) -> HealthSnapshot {
    let mut snapshot = HealthSnapshot {
        total: u32::try_from(records.len()).expect("record count fits u32"),
        ..HealthSnapshot::default()
    };
    for record in records {
        match record.health {
            VmHealth::Degraded => snapshot.degraded += 1,
            VmHealth::Exited => snapshot.exited += 1,
            VmHealth::Healthy => snapshot.healthy += 1,
            VmHealth::Stuck => snapshot.stuck += 1,
        }
    }
    snapshot.rollout_ready = snapshot.degraded == 0 && snapshot.exited == 0 && snapshot.stuck == 0;
    snapshot
}

/// Render a JSON health snapshot.
pub fn render_health_json(health: &HealthSnapshot) -> String {
    serde_json::to_string_pretty(health).unwrap()
}

fn is_allowed_metric_label_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-' | b':' | b',')
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn record(health: VmHealth) -> VmProbeRecord {
        VmProbeRecord {
            health,
            run_dir: PathBuf::from("/run/m80/vm"),
            vm_id: "vm".to_owned(),
            ownership_pid: None,
            owner_live: false,
            api_socket_visible: false,
            vsock_socket_visible: false,
            diagnostics_visible: false,
        }
    }

    #[test]
    fn aggregate_counts_each_health_class() {
        let snapshot = aggregate_health(&[
            record(VmHealth::Healthy),
            record(VmHealth::Degraded),
            record(VmHealth::Stuck),
            record(VmHealth::Exited),
        ]);
        assert_eq!(snapshot.healthy, 1);
        assert_eq!(snapshot.degraded, 1);
        assert_eq!(snapshot.stuck, 1);
        assert_eq!(snapshot.exited, 1);
        assert_eq!(snapshot.total, 4);
        assert!(!snapshot.rollout_ready);
    }

    #[test]
    fn render_health_json_is_pretty_json() {
        let snapshot = HealthSnapshot {
            healthy: 2,
            total: 2,
            rollout_ready: true,
            ..HealthSnapshot::default()
        };
        let value: serde_json::Value =
            serde_json::from_str(&render_health_json(&snapshot)).unwrap();
        assert_eq!(value["healthy"], 2);
        assert_eq!(value["rollout_ready"], true);
    }

    #[test]
    fn metric_label_value_rejects_free_string_shapes() {
        assert_eq!(MetricLabelValue::new(""), Err(MetricLabelError::Empty));
        assert!(matches!(
            MetricLabelValue::new("template/../../escape"),
            Err(MetricLabelError::InvalidByte { .. })
        ));
        assert!(matches!(
            MetricLabelValue::new("quoted\"value"),
            Err(MetricLabelError::InvalidByte { .. })
        ));
        assert_eq!(
            MetricLabelValue::new("sha256:abcd,sha256:ef01")
                .unwrap()
                .as_str(),
            "sha256:abcd,sha256:ef01"
        );
    }
}
