use std::io::Cursor;

use m80_proto::{
    read_frame, write_frame, Envelope, ShutdownAction, ShutdownRequest, ShutdownResponse,
    PAYLOAD_KIND_SHUTDOWN_REQUEST, PAYLOAD_KIND_SHUTDOWN_RESPONSE,
};

#[test]
fn shutdown_request_round_trips_optional_reason() {
    let env = Envelope::with_request_id(
        ShutdownRequest {
            reason: Some("normal stop".to_owned()),
        },
        "req-shutdown".to_owned(),
    );

    let mut bytes = Vec::new();
    write_frame(&mut bytes, &env).expect("write shutdown request");
    let decoded: Envelope<ShutdownRequest> =
        read_frame(&mut Cursor::new(bytes)).expect("read shutdown request");

    assert_eq!(decoded.kind, PAYLOAD_KIND_SHUTDOWN_REQUEST);
    assert_eq!(decoded.request_id.as_deref(), Some("req-shutdown"));
    assert_eq!(decoded.payload.reason.as_deref(), Some("normal stop"));
}

#[test]
fn shutdown_response_round_trips_all_actions() {
    for action in [ShutdownAction::Exit, ShutdownAction::Poweroff] {
        let env = Envelope::with_request_id(ShutdownResponse { action }, "req-shutdown".to_owned());

        let mut bytes = Vec::new();
        write_frame(&mut bytes, &env).expect("write shutdown response");
        let decoded: Envelope<ShutdownResponse> =
            read_frame(&mut Cursor::new(bytes)).expect("read shutdown response");

        assert_eq!(decoded.kind, PAYLOAD_KIND_SHUTDOWN_RESPONSE);
        assert_eq!(decoded.request_id.as_deref(), Some("req-shutdown"));
        assert_eq!(decoded.payload.action, action);
    }
}
