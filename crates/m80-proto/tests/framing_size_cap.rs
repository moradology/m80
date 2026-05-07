//! Strict `>` size cap at 4 MiB for protobuf frame bodies.

use std::io::Cursor;

use m80_proto::{read_frame, ExecRequest, ProtoError, MAX_FRAME_BYTES};

#[test]
fn rejects_frame_above_4mib_with_oversized_error() {
    let mut exact = (MAX_FRAME_BYTES as u32).to_be_bytes().to_vec();
    exact.resize(4 + MAX_FRAME_BYTES, 0);
    let mut cursor = Cursor::new(exact);
    let err = read_frame::<_, ExecRequest>(&mut cursor)
        .expect_err("exactly-MAX_FRAME_BYTES passes size gate then fails protobuf decode");
    assert!(
        matches!(err, ProtoError::MalformedPayload(_)),
        "exactly-MAX_FRAME_BYTES must not be treated as oversized; got {err:?}"
    );

    let mut over = ((MAX_FRAME_BYTES + 1) as u32).to_be_bytes().to_vec();
    over.extend_from_slice(b"ignored");
    let mut cursor2 = Cursor::new(over);
    let err: ProtoError = read_frame::<_, ExecRequest>(&mut cursor2)
        .expect_err("MAX_FRAME_BYTES+1 frame must be rejected");

    match err {
        ProtoError::OversizedPayload { size, limit } => {
            assert_eq!(size, MAX_FRAME_BYTES + 1);
            assert_eq!(limit, MAX_FRAME_BYTES);
        }
        other => panic!("expected OversizedPayload, got: {other:?}"),
    }
}
