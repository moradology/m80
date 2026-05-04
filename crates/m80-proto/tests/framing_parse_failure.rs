//! Bead m80-g3x.2.3 — malformed JSON returns `MalformedPayload`.
//!
//! Drop-connection semantics: after a parse failure the stream is
//! unrecoverable. `read_frame` itself doesn't enforce this — it only
//! reports the error. The actual connection-close lives at the handler
//! level (m80-guestd, m80-firecracker), which closes the socket on any
//! `read_frame` error. These tests verify the error surface that lets
//! handlers act on it.

use std::io::Cursor;

use m80_proto::{read_frame, Envelope, ExecRequest, ProtoError};

#[test]
fn malformed_json_returns_error_and_drops_connection() {
    let malformed = b"{not valid json\n";
    let mut cursor = Cursor::new(malformed.as_ref());

    let err: ProtoError = read_frame::<_, Envelope<ExecRequest>>(&mut cursor)
        .expect_err("malformed JSON must return an error");
    assert!(
        matches!(err, ProtoError::MalformedPayload(_)),
        "expected MalformedPayload, got: {err:?}"
    );

    // After a parse failure, a second read against the same exhausted
    // cursor must not return a successful frame.
    let second_result: Result<Envelope<ExecRequest>, ProtoError> = read_frame(&mut cursor);
    match second_result {
        Err(ProtoError::MalformedPayload(_)) | Err(ProtoError::Io(_)) => {}
        Ok(_) => panic!("stream must not recover after a parse failure"),
        Err(other) => panic!("unexpected error on second read: {other:?}"),
    }

    // A valid frame following a malformed one in the same buffer also
    // surfaces the parse error first; the handler is responsible for
    // closing rather than skipping ahead.
    let malformed_then_valid: &[u8] =
        b"{bad json\n{\"version\":1,\"payload\":{\"program\":\"/bin/true\",\"args\":[]}}\n";
    let mut cursor2 = Cursor::new(malformed_then_valid);
    let first: Result<Envelope<ExecRequest>, ProtoError> = read_frame(&mut cursor2);
    assert!(
        matches!(first, Err(ProtoError::MalformedPayload(_))),
        "first frame must surface as MalformedPayload"
    );
}
