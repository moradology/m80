//! Each envelope serializes as one length-prefixed protobuf frame.

mod common;

use std::io::{Cursor, Read};

use m80_proto::{read_frame, write_frame, Envelope, ExecRequest};

fn make_request(program: &str) -> Envelope<ExecRequest> {
    let mut req = common::sample_request();
    req.program = program.into();
    req.args = Vec::new();
    req.timeout_ms = None;
    Envelope::new(req)
}

fn read_len(cursor: &mut Cursor<&Vec<u8>>) -> usize {
    let mut prefix = [0u8; 4];
    cursor.read_exact(&mut prefix).unwrap();
    u32::from_be_bytes(prefix) as usize
}

#[test]
fn serializes_one_length_prefixed_frame_per_envelope() {
    let e1 = make_request("/bin/true");
    let e2 = make_request("/bin/false");
    let e3 = make_request("/bin/echo");

    let mut buf = Vec::new();
    write_frame(&mut buf, &e1).unwrap();
    write_frame(&mut buf, &e2).unwrap();
    write_frame(&mut buf, &e3).unwrap();

    let mut prefix_cursor = Cursor::new(&buf);
    for _ in 0..3 {
        let len = read_len(&mut prefix_cursor);
        assert!(len > 0, "protobuf frame body must not be empty");
        prefix_cursor.set_position(prefix_cursor.position() + len as u64);
    }
    assert_eq!(prefix_cursor.position() as usize, buf.len());

    let mut cursor = Cursor::new(&buf);
    let r1: Envelope<ExecRequest> = read_frame(&mut cursor).unwrap();
    let r2: Envelope<ExecRequest> = read_frame(&mut cursor).unwrap();
    let r3: Envelope<ExecRequest> = read_frame(&mut cursor).unwrap();
    assert_eq!(r1.payload.program, "/bin/true");
    assert_eq!(r2.payload.program, "/bin/false");
    assert_eq!(r3.payload.program, "/bin/echo");
}
