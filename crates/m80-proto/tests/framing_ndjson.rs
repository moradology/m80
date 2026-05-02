//! Bead m80-g3x.2.1 — each envelope serializes as exactly one NDJSON record.

mod common;

use std::io::Cursor;

use m80_proto::{Envelope, ExecRequest, read_frame, write_frame};

fn make_request(program: &str) -> Envelope<ExecRequest> {
    let mut req = common::sample_request();
    req.program = program.into();
    req.args = Vec::new();
    req.timeout_ms = None;
    Envelope::new(req)
}

#[test]
fn serializes_one_record_per_line() {
    let e1 = make_request("/bin/true");
    let e2 = make_request("/bin/false");
    let e3 = make_request("/bin/echo");

    let mut buf = Vec::new();
    write_frame(&mut buf, &e1).unwrap();
    write_frame(&mut buf, &e2).unwrap();
    write_frame(&mut buf, &e3).unwrap();

    let newline_count = buf.iter().filter(|&&b| b == b'\n').count();
    assert_eq!(newline_count, 3, "expected exactly 3 newlines for 3 frames");
    assert_eq!(*buf.last().unwrap(), b'\n');

    let content = std::str::from_utf8(&buf).unwrap();
    let lines: Vec<&str> = content.split('\n').collect();
    assert_eq!(lines.len(), 4);
    assert_eq!(lines[3], "");
    for line in &lines[..3] {
        let _: serde_json::Value = serde_json::from_str(line).unwrap();
    }

    let mut cursor = Cursor::new(&buf);
    let r1: Envelope<ExecRequest> = read_frame(&mut cursor).unwrap();
    let r2: Envelope<ExecRequest> = read_frame(&mut cursor).unwrap();
    let r3: Envelope<ExecRequest> = read_frame(&mut cursor).unwrap();
    assert_eq!(r1.payload.program, "/bin/true");
    assert_eq!(r2.payload.program, "/bin/false");
    assert_eq!(r3.payload.program, "/bin/echo");
}
