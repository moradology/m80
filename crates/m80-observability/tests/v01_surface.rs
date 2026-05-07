use m80_observability::{
    aggregate_health, probe, render_health_json, render_prometheus, Diagnostics, EventKind,
    HealthSnapshot, OpsMetrics, Phase, SourceClass, VmEvent, DIAGNOSTICS_SCHEMA_VERSION,
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
fn probe_empty_missing_run_root_returns_no_records() {
    let result = probe(std::path::Path::new("/nonexistent"));
    assert!(result.unwrap().is_empty());
}

#[test]
fn aggregate_health_empty_records_is_rollout_ready() {
    let snapshot = aggregate_health(&[]).unwrap();
    assert_eq!(snapshot.total, 0);
    assert!(snapshot.rollout_ready);
}

#[test]
fn render_prometheus_returns_exposition_text() {
    let snapshot = HealthSnapshot::default();
    let metrics = OpsMetrics::default();
    let result = render_prometheus(&snapshot, &metrics);
    assert!(result.contains("m80_vm_health_total"));
}

#[test]
fn render_health_json_returns_json() {
    let snapshot = HealthSnapshot::default();
    let result = render_health_json(&snapshot);
    let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(parsed["total"], 0);
}

#[test]
fn vm_event_roundtrips_through_serde_json() {
    let event = VmEvent {
        schema_version: DIAGNOSTICS_SCHEMA_VERSION,
        timestamp_unix_ms: 1_700_000_000_000,
        event_kind: EventKind::Lifecycle,
        source_class: SourceClass::Host,
        phase: Phase::Boot,
        message: "booting".to_string(),
        request_id: Some("req-1".to_string()),
        context: Default::default(),
        duration_us: None,
        outcome: None,
        exit_reason: None,
    };
    let json = serde_json::to_string(&event).expect("serialize");
    let back: VmEvent = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back, event);
}
