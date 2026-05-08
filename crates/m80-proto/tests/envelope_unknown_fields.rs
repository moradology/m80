use m80_proto::{read_raw_frame, ProtoError, PROTOCOL_VERSION};

#[test]
fn unknown_top_level_envelope_field_fails_closed_with_field_number() {
    let mut body = Vec::new();
    write_varint(&mut body, 1 << 3);
    write_varint(&mut body, u64::from(PROTOCOL_VERSION));
    write_varint(&mut body, (255 << 3) | 2);
    write_varint(&mut body, 0);

    let mut frame = Vec::new();
    frame.extend_from_slice(&(body.len() as u32).to_be_bytes());
    frame.extend_from_slice(&body);

    let err = read_raw_frame(&mut std::io::Cursor::new(frame)).unwrap_err();

    assert!(
        matches!(err, ProtoError::MalformedPayload(ref msg) if msg.contains("unknown envelope field: 255")),
        "unexpected error: {err:?}"
    );
}

fn write_varint(out: &mut Vec<u8>, mut value: u64) {
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        out.push(byte);
        if value == 0 {
            break;
        }
    }
}
