use std::io::Cursor;

use m80_proto::{
    read_frame, write_frame, Envelope, PingRequest, PongResponse, PAYLOAD_KIND_PING_REQUEST,
    PAYLOAD_KIND_PONG_RESPONSE,
};

#[test]
fn ping_request_round_trips() {
    let env = Envelope::with_request_id(PingRequest {}, "req-ping".to_owned());
    assert_eq!(env.kind, PAYLOAD_KIND_PING_REQUEST);

    let mut bytes = Vec::new();
    write_frame(&mut bytes, &env).unwrap();

    let decoded: Envelope<PingRequest> = read_frame(&mut Cursor::new(bytes)).unwrap();
    assert_eq!(decoded.kind, PAYLOAD_KIND_PING_REQUEST);
    assert_eq!(decoded.request_id.as_deref(), Some("req-ping"));
}

#[test]
fn pong_response_round_trips() {
    let env = Envelope::with_request_id(PongResponse { guest_unix_ms: 42 }, "req-ping".to_owned());
    assert_eq!(env.kind, PAYLOAD_KIND_PONG_RESPONSE);

    let mut bytes = Vec::new();
    write_frame(&mut bytes, &env).unwrap();

    let decoded: Envelope<PongResponse> = read_frame(&mut Cursor::new(bytes)).unwrap();
    assert_eq!(decoded.kind, PAYLOAD_KIND_PONG_RESPONSE);
    assert_eq!(decoded.request_id.as_deref(), Some("req-ping"));
    assert_eq!(decoded.payload.guest_unix_ms, 42);
}
