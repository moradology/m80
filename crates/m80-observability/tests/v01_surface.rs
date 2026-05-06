use m80_observability::{
    aggregate_health, probe, render_health_json, render_prometheus, Diagnostics, HealthSnapshot,
    ObservabilityError, OpsMetrics, Phase, SourceClass, VmEvent, DIAGNOSTICS_SCHEMA_VERSION,
};

fn sample_event() -> VmEvent {
    VmEvent::host(Phase::Boot, "test", None::<String>, Default::default())
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
        schema_version: DIAGNOSTICS_SCHEMA_VERSION,
        timestamp_unix_ms: 1_700_000_000_000,
        source_class: SourceClass::Host,
        phase: Phase::Boot,
        message: "booting".to_string(),
        request_id: Some("req-1".to_string()),
        context: Default::default(),
    };
    let json = serde_json::to_string(&event).expect("serialize");
    let back: VmEvent = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back, event);
}
