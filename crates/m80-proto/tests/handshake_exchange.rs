//! Reserved handshake payload keeps exact version negotiation semantics.

use std::io::Cursor;

use m80_proto::{
    read_frame, write_frame, Envelope, ExecRequest, HandshakeMessage, PROTOCOL_VERSION,
};

#[test]
fn reserved_handshake_payload_round_trips_exact_version() {
    let mut channel: Vec<u8> = Vec::new();
    let host_shake = HandshakeMessage {
        version: PROTOCOL_VERSION,
    };
    write_frame(&mut channel, &Envelope::new(host_shake)).unwrap();

    let mut reader = Cursor::new(&channel);
    let remote: Envelope<HandshakeMessage> = read_frame(&mut reader).unwrap();
    assert_eq!(
        remote.payload.version,
        PROTOCOL_VERSION,
        "same-version negotiation must succeed"
    );
}

#[test]
fn application_envelope_carries_version_without_handshake_prelude() {
    let req_env = Envelope::new(ExecRequest {
        program: "/bin/true".into(),
        args: Vec::new(),
        cwd: None,
        env: None,
        stdin: None,
        timeout_ms: None,
        streaming: false,
    });
    let mut app_channel: Vec<u8> = Vec::new();
    write_frame(&mut app_channel, &req_env).unwrap();

    let mut app_reader = Cursor::new(&app_channel);
    let decoded: Envelope<ExecRequest> = read_frame(&mut app_reader).unwrap();
    assert_eq!(decoded.payload.program, "/bin/true");
    assert_eq!(decoded.version, PROTOCOL_VERSION);
}
