//! `InstanceAction` serializes to the expected
//! `{"action_type": "..."}` JSON that Firecracker expects on PUT `/actions`.

mod fixture_server;

use fixture_server::setup_with_204;
use m80_firecracker_client::InstanceAction;

#[test]
fn instance_start_serializes_to_pascal_case() {
    let text =
        serde_json::to_string(&serde_json::json!({ "action_type": InstanceAction::InstanceStart }))
            .unwrap();
    assert_eq!(text, "{\"action_type\":\"InstanceStart\"}");
}

#[test]
fn instance_start_round_trips_through_json() {
    let serialized = serde_json::to_string(&InstanceAction::InstanceStart).unwrap();
    let deserialized: InstanceAction = serde_json::from_str(&serialized).unwrap();
    assert_eq!(InstanceAction::InstanceStart, deserialized);
}

#[test]
fn instance_action_request_uses_typed_payload_shape() {
    let (server, client) = setup_with_204();

    client
        .instance_action(InstanceAction::InstanceStart)
        .unwrap();

    let result = server.join();
    assert!(result.request.starts_with("PUT /actions HTTP/1.1\r\n"));
    assert!(result
        .request
        .ends_with("\r\n\r\n{\"action_type\":\"InstanceStart\"}"));
}
