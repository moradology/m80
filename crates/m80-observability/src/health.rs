use serde::{Deserialize, Serialize};

use m80_proto::MetricsResponse;

use crate::probe::{VmHealth, VmProbeRecord};
use crate::ObservabilityError;

/// Aggregated rollup of probe records.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct HealthSnapshot {
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
pub(crate) struct OpsMetrics {
    /// Number of VMs the metrics span.
    pub vm_count: u32,
    /// Guest metrics sampled from one running VM at scrape time.
    pub guest: Option<MetricsResponse>,
}

/// Aggregate probe records into a [`HealthSnapshot`].
pub(crate) fn aggregate_health(records: &[VmProbeRecord]) -> Result<HealthSnapshot, ObservabilityError> {
    let mut snapshot = HealthSnapshot {
        total: records.len() as u32,
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
    Ok(snapshot)
}

/// Render a JSON health snapshot.
pub(crate) fn render_health_json(health: &HealthSnapshot) -> String {
    serde_json::to_string_pretty(health).unwrap()
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
        ])
        .unwrap();
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
}
