use m80_observability::{
    Diagnostics, HealthSnapshot, OpsMetrics, ObservabilityError, Phase, VmEvent,
    aggregate_health, probe, render_health_json, render_prometheus,
};

fn sample_event() -> VmEvent {
    VmEvent {
        detail: "test".to_string(),
        phase: Phase::Boot,
        timestamp_unix_ms: 0,
    }
}

#[test]
fn disabled_diagnostics_record_is_noop() {
    let mut d = Diagnostics::disabled();
    let result = d.record(&sample_event());
    assert!(result.is_ok());
}

#[test]
fn probe_returns_deferred_in_v01() {
    let result = probe(std::path::Path::new("/nonexistent"));
    assert!(matches!(result, Err(ObservabilityError::Deferred)));
}

#[test]
fn aggregate_health_returns_deferred_in_v01() {
    let result = aggregate_health(&[]);
    assert!(matches!(result, Err(ObservabilityError::Deferred)));
}

#[test]
fn render_prometheus_returns_deferred_in_v01() {
    let snapshot = HealthSnapshot::default();
    let metrics = OpsMetrics::default();
    let result = render_prometheus(&snapshot, &metrics);
    assert!(matches!(result, Err(ObservabilityError::Deferred)));
}

#[test]
fn render_health_json_returns_deferred_in_v01() {
    let snapshot = HealthSnapshot::default();
    let result = render_health_json(&snapshot);
    assert!(matches!(result, Err(ObservabilityError::Deferred)));
}

#[test]
fn vm_event_roundtrips_through_serde_json() {
    let event = VmEvent {
        detail: "booting".to_string(),
        phase: Phase::Boot,
        timestamp_unix_ms: 1_700_000_000_000,
    };
    let json = serde_json::to_string(&event).expect("serialize");
    let back: VmEvent = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.detail, event.detail);
    assert_eq!(back.phase, event.phase);
    assert_eq!(back.timestamp_unix_ms, event.timestamp_unix_ms);
}

#[test]
fn health_snapshot_default_is_zero() {
    let snap = HealthSnapshot::default();
    assert_eq!(snap.healthy, 0);
    assert_eq!(snap.degraded, 0);
    assert_eq!(snap.stuck, 0);
    assert_eq!(snap.exited, 0);
}
