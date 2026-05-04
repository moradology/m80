//! Each `InstanceAction` variant serializes to the expected
//! `{"action_type": "..."}` JSON that Firecracker expects on PUT `/actions`.

use m80_firecracker_client::InstanceAction;

#[test]
fn instance_actions_serialize_to_pascal_case() {
    for (action, expected) in [
        (InstanceAction::InstanceStart, "InstanceStart"),
        (InstanceAction::SendCtrlAltDel, "SendCtrlAltDel"),
        (InstanceAction::FlushMetrics, "FlushMetrics"),
        (InstanceAction::Pause, "Pause"),
        (InstanceAction::Resume, "Resume"),
    ] {
        let text = serde_json::to_string(&serde_json::json!({ "action_type": action })).unwrap();
        assert_eq!(text, format!("{{\"action_type\":\"{expected}\"}}"));
    }
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
