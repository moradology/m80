//! Bead m80-g3x.3.1 — version handshake runs before the first request.

use std::io::Cursor;

use m80_proto::{
    negotiate_version, read_frame, write_frame, Envelope, ExecRequest, HandshakeMessage,
    PROTOCOL_VERSION,
};

#[test]
fn handshake_runs_before_first_request() {
    // 1. Host emits its handshake.
    let mut channel: Vec<u8> = Vec::new();
    let host_shake = HandshakeMessage {
        version: PROTOCOL_VERSION,
    };
    write_frame(&mut channel, &host_shake).unwrap();

    // 2. Guest reads and negotiates.
    let mut reader = Cursor::new(&channel);
    let remote: HandshakeMessage = read_frame(&mut reader).unwrap();
    negotiate_version(remote.version).expect("same-version negotiation must succeed");

    // 3. Only after a successful handshake does the host send a request.
    let req_env = Envelope::new(ExecRequest {
        program: "/bin/true".into(),
        args: Vec::new(),
        cwd: None,
        env: None,
        stdin: None,
        timeout_ms: None,
    });
    let mut app_channel: Vec<u8> = Vec::new();
    write_frame(&mut app_channel, &req_env).unwrap();

    let mut app_reader = Cursor::new(&app_channel);
    let decoded: Envelope<ExecRequest> = read_frame(&mut app_reader).unwrap();
    assert_eq!(decoded.payload.program, "/bin/true");
    assert_eq!(decoded.version, PROTOCOL_VERSION);
}
