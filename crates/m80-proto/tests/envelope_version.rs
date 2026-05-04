//! Bead m80-g3x.1.1 — every envelope carries `version: u32 = PROTOCOL_VERSION`.

mod common;

use std::io::Cursor;

use m80_proto::{read_frame, write_frame, Envelope, ExecRequest, ExecResponse, PROTOCOL_VERSION};

#[test]
fn request_and_response_carry_protocol_version() {
    let req_env = Envelope::new(common::sample_request());
    assert_eq!(req_env.version, PROTOCOL_VERSION);

    let mut buf = Vec::new();
    write_frame(&mut buf, &req_env).unwrap();
    let decoded_req: Envelope<ExecRequest> = read_frame(&mut Cursor::new(&buf)).unwrap();
    assert_eq!(decoded_req.version, PROTOCOL_VERSION);

    let resp_env = Envelope::new(common::sample_response());
    assert_eq!(resp_env.version, PROTOCOL_VERSION);

    let mut buf = Vec::new();
    write_frame(&mut buf, &resp_env).unwrap();
    let decoded_resp: Envelope<ExecResponse> = read_frame(&mut Cursor::new(&buf)).unwrap();
    assert_eq!(decoded_resp.version, PROTOCOL_VERSION);
}
