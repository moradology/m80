//! Each `InstanceAction` variant serializes to the expected
//! `{"action_type": "..."}` JSON that Firecracker expects on PUT `/actions`.

use m80_firecracker_client::InstanceAction;

/// Assert that `{"action_type": <expected>}` is produced for an action.
fn assert_action_type(action: InstanceAction, expected_variant: &str) {
    let json = serde_json::json!({ "action_type": action });
    let text = serde_json::to_string(&json).unwrap();
    let expected = format!("{{\"action_type\":\"{expected_variant}\"}}");
    assert_eq!(text, expected, "action_type mismatch for {action:?}");
}

#[test]
fn instance_start_serializes_to_pascal_case() {
    assert_action_type(InstanceAction::InstanceStart, "InstanceStart");
}

#[test]
fn send_ctrl_alt_del_serializes_to_pascal_case() {
    assert_action_type(InstanceAction::SendCtrlAltDel, "SendCtrlAltDel");
}

#[test]
fn flush_metrics_serializes_to_pascal_case() {
    assert_action_type(InstanceAction::FlushMetrics, "FlushMetrics");
}

#[test]
fn pause_serializes_to_pascal_case() {
    assert_action_type(InstanceAction::Pause, "Pause");
}

#[test]
fn resume_serializes_to_pascal_case() {
    assert_action_type(InstanceAction::Resume, "Resume");
}

#[test]
fn instance_action_round_trips_through_json() {
    for action in [
        InstanceAction::InstanceStart,
        InstanceAction::SendCtrlAltDel,
        InstanceAction::FlushMetrics,
        InstanceAction::Pause,
        InstanceAction::Resume,
    ] {
        let serialized = serde_json::to_string(&action).unwrap();
        let deserialized: InstanceAction = serde_json::from_str(&serialized).unwrap();
        assert_eq!(action, deserialized, "round-trip failed for {action:?}");
    }
}
