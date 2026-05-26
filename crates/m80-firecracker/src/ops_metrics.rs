//! Process-local operational counters exported through `m80-observability`.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use m80_observability::{
    ErrorCount, MetricLabelValue, OpsMetrics, PhaseFailureCount, WarmPoolMetrics,
};

use crate::warm_pool::WarmPoolSnapshot;

static OPS_COUNTERS: OnceLock<OpsCounterStore> = OnceLock::new();

struct OpsCounterStore {
    launches_total: AtomicU64,
    vsock_disconnects_total: AtomicU64,
    idle_timeout_total: AtomicU64,
    errors_total: Mutex<BTreeMap<String, Arc<AtomicU64>>>,
    phase_failures_total: Mutex<BTreeMap<String, Arc<AtomicU64>>>,
}

impl OpsCounterStore {
    fn new() -> Self {
        Self {
            launches_total: AtomicU64::new(0),
            vsock_disconnects_total: AtomicU64::new(0),
            idle_timeout_total: AtomicU64::new(0),
            errors_total: Mutex::new(BTreeMap::new()),
            phase_failures_total: Mutex::new(BTreeMap::new()),
        }
    }

    fn increment_labeled(map: &Mutex<BTreeMap<String, Arc<AtomicU64>>>, label: &str) {
        let counter = {
            let mut guard = map.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            guard
                .entry(label.to_owned())
                .or_insert_with(|| Arc::new(AtomicU64::new(0)))
                .clone()
        };
        counter.fetch_add(1, Ordering::Relaxed);
    }

    fn snapshot_labeled(
        map: &Mutex<BTreeMap<String, Arc<AtomicU64>>>,
    ) -> Vec<(MetricLabelValue, u64)> {
        let guard = map.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        guard
            .iter()
            .map(|(label, counter)| {
                (
                    MetricLabelValue::new(label.clone())
                        .expect("internal metric labels are fixed ASCII identifiers"),
                    counter.load(Ordering::Relaxed),
                )
            })
            .collect()
    }

    fn snapshot(&self) -> OpsMetrics {
        let errors_total = Self::snapshot_labeled(&self.errors_total)
            .into_iter()
            .map(|(variant, total)| ErrorCount { variant, total })
            .collect();
        let phase_failures_total = Self::snapshot_labeled(&self.phase_failures_total)
            .into_iter()
            .map(|(phase, total)| PhaseFailureCount { phase, total })
            .collect();

        OpsMetrics {
            launches_total: self.launches_total.load(Ordering::Relaxed),
            errors_total,
            phase_failures_total,
            vsock_disconnects_total: self.vsock_disconnects_total.load(Ordering::Relaxed),
            idle_timeout_total: self.idle_timeout_total.load(Ordering::Relaxed),
            ..OpsMetrics::default()
        }
    }

    #[cfg(test)]
    fn reset(&self) {
        self.launches_total.store(0, Ordering::Relaxed);
        self.vsock_disconnects_total.store(0, Ordering::Relaxed);
        self.idle_timeout_total.store(0, Ordering::Relaxed);
        self.errors_total
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clear();
        self.phase_failures_total
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clear();
    }
}

fn ops_counter_store() -> &'static OpsCounterStore {
    OPS_COUNTERS.get_or_init(OpsCounterStore::new)
}

/// Return a process-local operational metrics snapshot.
///
/// Counters reset when the embedding process exits. Persistent or fleet-wide
/// storage is intentionally left to the caller's scrape/aggregation layer.
#[must_use]
pub fn ops_metrics_snapshot() -> OpsMetrics {
    ops_counter_store().snapshot()
}

/// Convert a warm-pool snapshot into renderable operational metrics.
#[must_use]
pub fn warm_pool_metrics(snapshot: WarmPoolSnapshot) -> WarmPoolMetrics {
    WarmPoolMetrics {
        target_ready: usize_to_u64(snapshot.target_ready),
        ready: usize_to_u64(snapshot.ready),
        filling: usize_to_u64(snapshot.filling),
        leased: usize_to_u64(snapshot.leased),
        discarded_total: usize_to_u64(snapshot.discarded),
        consecutive_fill_errors: u64::from(snapshot.consecutive_fill_errors),
        fill_attempts_total: usize_to_u64(snapshot.fill_attempts_total),
        fill_failures_total: usize_to_u64(snapshot.fill_failures_total),
        lease_acquired_total: usize_to_u64(snapshot.lease_acquired_total),
        lease_returned_total: usize_to_u64(snapshot.lease_returned_total),
    }
}

pub(crate) fn record_launch() {
    ops_counter_store()
        .launches_total
        .fetch_add(1, Ordering::Relaxed);
}

pub(crate) fn record_phase_failure(phase_name: &str, error_variant: Option<&'static str>) {
    OpsCounterStore::increment_labeled(&ops_counter_store().phase_failures_total, phase_name);
    if let Some(error_variant) = error_variant {
        OpsCounterStore::increment_labeled(&ops_counter_store().errors_total, error_variant);
    }
}

pub(crate) fn record_vsock_disconnect() {
    ops_counter_store()
        .vsock_disconnects_total
        .fetch_add(1, Ordering::Relaxed);
}

pub(crate) fn record_idle_timeout() {
    ops_counter_store()
        .idle_timeout_total
        .fetch_add(1, Ordering::Relaxed);
}

fn usize_to_u64(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

#[cfg(test)]
pub(crate) fn reset_ops_metrics_for_test() {
    ops_counter_store().reset();
}

#[cfg(test)]
pub(crate) fn with_ops_metrics_test_lock<T>(f: impl FnOnce() -> T) -> T {
    static LOCK: Mutex<()> = Mutex::new(());
    let _guard = LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    f()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counters_snapshot_by_error_variant_and_phase() {
        with_ops_metrics_test_lock(|| {
            reset_ops_metrics_for_test();

            record_launch();
            record_launch();
            record_phase_failure("phase_3_storage_prep", Some("Storage"));
            record_phase_failure("phase_3_storage_prep", Some("Storage"));
            record_phase_failure("phase_6_network_realize", Some("Network"));
            record_vsock_disconnect();
            record_idle_timeout();

            let snapshot = ops_metrics_snapshot();
            assert_eq!(snapshot.launches_total, 2);
            assert_eq!(snapshot.vsock_disconnects_total, 1);
            assert_eq!(snapshot.idle_timeout_total, 1);
            assert_eq!(snapshot.errors_total.len(), 2);
            assert_eq!(snapshot.errors_total[0].variant.as_str(), "Network");
            assert_eq!(snapshot.errors_total[0].total, 1);
            assert_eq!(snapshot.errors_total[1].variant.as_str(), "Storage");
            assert_eq!(snapshot.errors_total[1].total, 2);
            assert_eq!(snapshot.phase_failures_total.len(), 2);
            assert_eq!(
                snapshot.phase_failures_total[0].phase.as_str(),
                "phase_3_storage_prep"
            );
            assert_eq!(snapshot.phase_failures_total[0].total, 2);
            reset_ops_metrics_for_test();
        });
    }

    #[test]
    fn warm_pool_snapshot_maps_to_metrics() {
        let metrics = warm_pool_metrics(WarmPoolSnapshot {
            target_ready: 2,
            ready: 1,
            filling: 1,
            leased: 3,
            discarded: 4,
            consecutive_fill_errors: 5,
            fill_attempts_total: 6,
            fill_failures_total: 7,
            lease_acquired_total: 8,
            lease_returned_total: 9,
        });

        assert_eq!(metrics.target_ready, 2);
        assert_eq!(metrics.ready, 1);
        assert_eq!(metrics.discarded_total, 4);
        assert_eq!(metrics.consecutive_fill_errors, 5);
        assert_eq!(metrics.lease_returned_total, 9);
    }
}
