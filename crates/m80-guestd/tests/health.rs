use std::io::Cursor;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use m80_proto::{
    read_frame, write_frame, Envelope, PingRequest, PongResponse, PAYLOAD_KIND_PONG_RESPONSE,
};

fn ping_frame(request_id: &str) -> Vec<u8> {
    let env = Envelope::with_request_id(PingRequest {}, request_id.to_owned());
    let mut buf = Vec::new();
    write_frame(&mut buf, &env).expect("write_frame in test");
    buf
}

fn run_handler(input: Vec<u8>) -> Vec<u8> {
    let mut out = Vec::new();
    m80_guestd::connection::handle_connection_with_reader_ready(
        std::io::BufReader::new(Cursor::new(input)),
        &mut out,
        |_| true,
    )
    .expect("handle_connection failed");
    out
}

fn unix_ms_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_millis() as u64
}

#[test]
fn ping_request_returns_pong_without_exec() {
    let before = unix_ms_now();
    let out = run_handler(ping_frame("req-ping"));
    let after = unix_ms_now();

    let mut cursor = Cursor::new(out);
    let response: Envelope<PongResponse> = read_frame(&mut cursor).unwrap();

    assert_eq!(response.kind, PAYLOAD_KIND_PONG_RESPONSE);
    assert_eq!(response.request_id.as_deref(), Some("req-ping"));
    assert!(
        (before..=after).contains(&response.payload.guest_unix_ms),
        "guest_unix_ms={} outside {before}..={after}",
        response.payload.guest_unix_ms
    );
}
