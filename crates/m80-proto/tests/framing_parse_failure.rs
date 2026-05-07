//! Malformed protobuf returns `MalformedPayload`.

use std::io::{self, Cursor};

use m80_proto::{
    read_frame, wire::generated::WireEnvelope, Envelope, ExecRequest, ProtoError, PROTOCOL_VERSION,
};
use prost::Message;

#[test]
fn malformed_protobuf_returns_error_and_drops_connection() {
    let mut malformed = 4u32.to_be_bytes().to_vec();
    malformed.extend_from_slice(&[0xff, 0xff, 0xff, 0xff]);
    let mut cursor = Cursor::new(malformed);

    let err: ProtoError = read_frame::<_, ExecRequest>(&mut cursor)
        .expect_err("malformed protobuf must return an error");
    assert!(
        matches!(err, ProtoError::MalformedPayload(_)),
        "expected MalformedPayload, got: {err:?}"
    );

    let second_result: Result<Envelope<ExecRequest>, ProtoError> = read_frame(&mut cursor);
    assert!(matches!(second_result, Err(ProtoError::Io(_))));
}

#[test]
fn short_frame_body_returns_unexpected_eof() {
    let mut short = 8u32.to_be_bytes().to_vec();
    short.extend_from_slice(&[0, 1, 2]);
    let mut cursor = Cursor::new(short);

    let err =
        read_frame::<_, ExecRequest>(&mut cursor).expect_err("short protobuf body must return EOF");

    assert!(matches!(err, ProtoError::Io(err) if err.kind() == io::ErrorKind::UnexpectedEof));
}

#[test]
fn unsupported_version_returns_incompatible_version() {
    let body = WireEnvelope {
        version: PROTOCOL_VERSION + 1,
        kind: "exec_request".to_owned(),
        request_id: None,
        max_duration_ms: None,
        payload: Some(
            m80_proto::wire::generated::wire_envelope::Payload::ExecRequest(
                m80_proto::wire::generated::WireExecRequest {
                    program: "/bin/true".to_owned(),
                    args: Vec::new(),
                    cwd: None,
                    env: Vec::new(),
                    stdin: None,
                    timeout_ms: None,
                    streaming: false,
                },
            ),
        ),
    }
    .encode_to_vec();
    let mut bytes = (body.len() as u32).to_be_bytes().to_vec();
    bytes.extend_from_slice(&body);

    let err = read_frame::<_, ExecRequest>(&mut Cursor::new(bytes))
        .expect_err("unsupported version must fail closed");

    assert!(matches!(
        err,
        ProtoError::IncompatibleVersion { expected, got }
            if expected == PROTOCOL_VERSION && got == PROTOCOL_VERSION + 1
    ));
}
