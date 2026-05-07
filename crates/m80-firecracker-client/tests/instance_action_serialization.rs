//! `InstanceAction` serializes to the expected
//! `{"action_type": "..."}` JSON that Firecracker expects on PUT `/actions`.

use m80_firecracker_client::InstanceAction;

#[test]
fn instance_start_serializes_to_pascal_case() {
    let text = serde_json::to_string(&serde_json::json!({ "action_type": InstanceAction::InstanceStart })).unwrap();
    assert_eq!(text, "{\"action_type\":\"InstanceStart\"}");
}

#[test]
fn instance_start_round_trips_through_json() {
    let serialized = serde_json::to_string(&InstanceAction::InstanceStart).unwrap();
    let deserialized: InstanceAction = serde_json::from_str(&serialized).unwrap();
    assert_eq!(InstanceAction::InstanceStart, deserialized);
}
