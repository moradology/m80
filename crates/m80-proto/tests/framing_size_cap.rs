//! Bead m80-g3x.2.2 — strict `>` size cap at 4 MiB.

use std::io::Cursor;

use m80_proto::{read_frame, Envelope, ExecRequest, ProtoError, MAX_FRAME_BYTES};

/// Build a raw NDJSON line of exactly `target_len` bytes (not counting `\n`)
/// by embedding enough `'a'` bytes into the `program` field.
///
/// The envelope skeleton is:
/// `{"version":1,"payload":{"program":"<AAAA...>","args":[],"timeout_ms":null}}`
///
/// We compute the skeleton length (without the padding), then fill the
/// `program` string with as many `'a'` bytes as needed to reach `target_len`.
fn raw_line_of_len(target_len: usize) -> Vec<u8> {
    // Build an envelope with an empty program, serialize it, measure overhead.
    let env = Envelope::new(ExecRequest {
        program: String::new(),
        args: Vec::new(),
        cwd: None,
        env: None,
        stdin: None,
        timeout_ms: None,
        streaming: false,
    });
    let base = serde_json::to_vec(&env).expect("serialization cannot fail");
    // base has `"program":""` — the empty string is 2 chars ("").
    // Each 'a' we add grows the payload by exactly 1 byte.
    let base_len = base.len();
    assert!(
        target_len >= base_len,
        "target_len {target_len} is smaller than the base envelope size {base_len}"
    );
    let padding = target_len - base_len;
    let padded_env = Envelope::new(ExecRequest {
        program: "a".repeat(padding),
        args: Vec::new(),
        cwd: None,
        env: None,
        stdin: None,
        timeout_ms: None,
        streaming: false,
    });
    let serialized = serde_json::to_vec(&padded_env).expect("serialization cannot fail");
    assert_eq!(
        serialized.len(),
        target_len,
        "constructed frame is wrong length"
    );
    serialized
}

#[test]
fn rejects_frame_above_4mib_with_oversized_error() {
    // Exactly MAX_FRAME_BYTES — must pass the size gate.
    let exact = raw_line_of_len(MAX_FRAME_BYTES);
    assert_eq!(exact.len(), MAX_FRAME_BYTES);

    // Write it as an NDJSON line (append `\n`) and feed to read_frame.
    let mut buf_exact = exact.clone();
    buf_exact.push(b'\n');

    let mut cursor = Cursor::new(&buf_exact);
    let result: Result<Envelope<ExecRequest>, ProtoError> = read_frame(&mut cursor);
    // The size gate passes. The parse will succeed (the frame is valid JSON).
    assert!(
        result.is_ok(),
        "exactly-MAX_FRAME_BYTES frame must pass the size gate; got: {result:?}"
    );

    // MAX_FRAME_BYTES + 1 — must return OversizedPayload.
    // Construct by directly appending one byte so read_frame sees it without
    // write_frame's own gate interfering.
    let mut over = raw_line_of_len(MAX_FRAME_BYTES);
    // Append one extra byte inside the JSON string region (still UTF-8 safe).
    // We insert 'a' before the closing quote of `program`.
    // Simplest: just append a byte that makes the trimmed line 1 byte longer.
    // We do this by building a raw byte sequence: valid JSON is not required
    // for an oversized rejection — the size check fires first.
    over.push(b'x'); // now len == MAX_FRAME_BYTES + 1
    over.push(b'\n'); // NDJSON terminator

    let mut cursor2 = Cursor::new(&over);
    let err: ProtoError = read_frame::<_, Envelope<ExecRequest>>(&mut cursor2)
        .expect_err("MAX_FRAME_BYTES+1 frame must be rejected");

    match err {
        ProtoError::OversizedPayload { size, limit } => {
            assert_eq!(
                size,
                MAX_FRAME_BYTES + 1,
                "reported size must equal MAX_FRAME_BYTES + 1"
            );
            assert_eq!(
                limit, MAX_FRAME_BYTES,
                "reported limit must equal MAX_FRAME_BYTES"
            );
        }
        other => panic!("expected OversizedPayload, got: {other:?}"),
    }
}
