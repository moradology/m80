//! NDJSON framing: `read_frame`, `write_frame`, version-probe.

use std::io::{self, BufRead, Read, Write};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::error::ProtoError;
use crate::version::{MAX_FRAME_BYTES, PROTOCOL_VERSION};

/// Partial version probe; intentionally allows unknown fields so wrong-version
/// frames report `IncompatibleVersion` before full strict deserialization.
#[derive(Deserialize)]
struct VersionProbe {
    version: u32,
}

/// Read one NDJSON-framed value from `reader`.
///
/// `T` must have a top-level `version: u32` field — both `Envelope<T>` and
/// `HandshakeMessage` qualify. The version is extracted via a partial parse
/// before the full deserialize, so a wrong-version frame fails as
/// [`ProtoError::IncompatibleVersion`] rather than as a structural-shape
/// error against `T`.
pub fn read_frame<R, T>(reader: &mut R) -> Result<T, ProtoError>
where
    R: BufRead,
    T: DeserializeOwned,
{
    let mut buf: Vec<u8> = Vec::with_capacity(256);
    let n_read = reader
        .by_ref()
        .take(MAX_FRAME_BYTES as u64 + 2)
        .read_until(b'\n', &mut buf)?;

    if n_read == 0 {
        return Err(ProtoError::Io(io::Error::from(
            io::ErrorKind::UnexpectedEof,
        )));
    }

    let found_newline = buf.last().copied() == Some(b'\n');
    if !found_newline {
        return if n_read == MAX_FRAME_BYTES + 2 {
            Err(ProtoError::OversizedPayload {
                size: n_read,
                limit: MAX_FRAME_BYTES,
            })
        } else {
            Err(ProtoError::Io(io::Error::from(
                io::ErrorKind::UnexpectedEof,
            )))
        };
    }

    buf.pop(); // strip trailing `\n`

    if buf.len() > MAX_FRAME_BYTES {
        return Err(ProtoError::OversizedPayload {
            size: buf.len(),
            limit: MAX_FRAME_BYTES,
        });
    }

    let probe: VersionProbe = serde_json::from_slice(&buf).map_err(ProtoError::MalformedPayload)?;
    if probe.version != PROTOCOL_VERSION {
        return Err(ProtoError::IncompatibleVersion {
            expected: PROTOCOL_VERSION,
            got: probe.version,
        });
    }

    serde_json::from_slice(&buf).map_err(ProtoError::MalformedPayload)
}

/// Write one NDJSON-framed value to `writer`, including the trailing `\n`.
///
/// Returns [`ProtoError::OversizedPayload`] if the serialized frame exceeds
/// [`MAX_FRAME_BYTES`].
pub fn write_frame<W, T>(writer: &mut W, value: &T) -> Result<(), ProtoError>
where
    W: Write,
    T: Serialize,
{
    let buf = serde_json::to_vec(value).map_err(ProtoError::EncodeFailed)?;
    if buf.len() > MAX_FRAME_BYTES {
        return Err(ProtoError::OversizedPayload {
            size: buf.len(),
            limit: MAX_FRAME_BYTES,
        });
    }
    writer.write_all(&buf)?;
    writer.write_all(b"\n")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Envelope, ExecRequest, ExecResponse, ExecStatus, ExecTiming};
    use std::io::Cursor;

    fn sample_timing() -> ExecTiming {
        ExecTiming {
            spawned_at_unix_ms: 1_000_000,
            exited_at_unix_ms: 1_000_100,
            spawn_ms: 10,
            run_ms: 90,
        }
    }

    fn sample_request() -> ExecRequest {
        ExecRequest {
            program: "/bin/sh".into(),
            args: vec!["-c".into(), "echo hi".into()],
            cwd: None,
            env: None,
            stdin: None,
            timeout_ms: Some(5_000),
            streaming: false,
        }
    }

    fn sample_response() -> ExecResponse {
        ExecResponse {
            status: ExecStatus::Completed,
            exit_code: Some(0),
            stdout: b"hello\n".to_vec(),
            stderr: Vec::new(),
            truncated: None,
            timing: sample_timing(),
        }
    }

    #[test]
    fn request_round_trip() {
        let env = Envelope::new(sample_request());
        let mut buf = Vec::new();
        write_frame(&mut buf, &env).unwrap();
        assert_eq!(*buf.last().unwrap(), b'\n');

        let mut cursor = Cursor::new(&buf);
        let back: Envelope<ExecRequest> = read_frame(&mut cursor).unwrap();
        assert_eq!(back, env);
    }

    #[test]
    fn response_round_trip() {
        let env = Envelope::new(sample_response());
        let mut buf = Vec::new();
        write_frame(&mut buf, &env).unwrap();
        let mut cursor = Cursor::new(&buf);
        let back: Envelope<ExecResponse> = read_frame(&mut cursor).unwrap();
        assert_eq!(back, env);
    }

    #[test]
    fn read_frame_empty_stream_returns_unexpected_eof() {
        let mut cursor = Cursor::new(Vec::<u8>::new());
        let err = read_frame::<_, Envelope<ExecRequest>>(&mut cursor).unwrap_err();
        assert!(matches!(err, ProtoError::Io(e) if e.kind() == io::ErrorKind::UnexpectedEof));
    }

    #[test]
    fn read_frame_unbounded_input_caps_at_limit() {
        let oversized = vec![b'a'; MAX_FRAME_BYTES + 100];
        let mut cursor = Cursor::new(oversized);
        let err = read_frame::<_, Envelope<ExecRequest>>(&mut cursor).unwrap_err();
        assert!(matches!(err, ProtoError::OversizedPayload { .. }));
    }

    /// Peer closed mid-frame (read bytes < cap, no `\n`) must surface as
    /// `Io(UnexpectedEof)`, NOT `OversizedPayload`.
    #[test]
    fn read_frame_truncated_input_returns_unexpected_eof() {
        let truncated = b"{\"version\":1,\"payload\":".to_vec();
        let mut cursor = Cursor::new(truncated);
        let err = read_frame::<_, Envelope<ExecRequest>>(&mut cursor).unwrap_err();
        assert!(
            matches!(&err, ProtoError::Io(e) if e.kind() == io::ErrorKind::UnexpectedEof),
            "truncated frame must surface as UnexpectedEof; got {err:?}"
        );
    }

    #[test]
    fn write_frame_rejects_oversize_envelope() {
        let big = ExecResponse {
            status: ExecStatus::Completed,
            exit_code: Some(0),
            stdout: vec![b'x'; (MAX_FRAME_BYTES * 7) / 8],
            stderr: Vec::new(),
            truncated: None,
            timing: sample_timing(),
        };
        let env = Envelope::new(big);
        let mut buf = Vec::new();
        let err = write_frame(&mut buf, &env).unwrap_err();
        assert!(matches!(err, ProtoError::OversizedPayload { size, limit }
            if size > MAX_FRAME_BYTES && limit == MAX_FRAME_BYTES));
    }

    #[test]
    fn all_byte_values_round_trip_in_stdout_and_stderr() {
        let all_bytes: Vec<u8> = (0u16..=255).map(|b| b as u8).collect();
        let env = Envelope::new(ExecResponse {
            status: ExecStatus::Completed,
            exit_code: Some(0),
            stdout: all_bytes.clone(),
            stderr: all_bytes.clone(),
            truncated: None,
            timing: sample_timing(),
        });
        let mut buf = Vec::new();
        write_frame(&mut buf, &env).unwrap();
        let mut cursor = Cursor::new(&buf);
        let back: Envelope<ExecResponse> = read_frame(&mut cursor).unwrap();
        assert_eq!(back.payload.stdout, all_bytes);
        assert_eq!(back.payload.stderr, all_bytes);
    }

    #[test]
    fn all_byte_values_round_trip_in_stdin_option() {
        let all_bytes: Vec<u8> = (0u16..=255).map(|b| b as u8).collect();
        let env = Envelope::new(ExecRequest {
            program: "/bin/cat".into(),
            args: vec![],
            cwd: None,
            env: None,
            stdin: Some(all_bytes.clone()),
            timeout_ms: None,
            streaming: false,
        });
        let mut buf = Vec::new();
        write_frame(&mut buf, &env).unwrap();
        let mut cursor = Cursor::new(&buf);
        let back: Envelope<ExecRequest> = read_frame(&mut cursor).unwrap();
        assert_eq!(back.payload.stdin, Some(all_bytes));
    }

    #[test]
    fn read_frame_rejects_extra_envelope_field() {
        let raw = format!(
            "{{\"version\":{ver},\"kind\":\"exec_request\",\"payload\":{{\"program\":\"/x\",\"args\":[]}},\"extra\":\"junk\"}}\n",
            ver = PROTOCOL_VERSION,
        );
        let mut cursor = Cursor::new(raw.into_bytes());
        let err = read_frame::<_, Envelope<ExecRequest>>(&mut cursor).unwrap_err();
        assert!(
            matches!(err, ProtoError::MalformedPayload(_)),
            "extra envelope field must surface as MalformedPayload; got {err:?}"
        );
    }

    #[test]
    fn read_frame_rejects_extra_payload_field() {
        let raw = format!(
            "{{\"version\":{ver},\"kind\":\"exec_request\",\"payload\":{{\"program\":\"/x\",\"args\":[],\"bogus\":42}}}}\n",
            ver = PROTOCOL_VERSION,
        );
        let mut cursor = Cursor::new(raw.into_bytes());
        let err = read_frame::<_, Envelope<ExecRequest>>(&mut cursor).unwrap_err();
        assert!(
            matches!(err, ProtoError::MalformedPayload(_)),
            "extra payload field must surface as MalformedPayload; got {err:?}"
        );
    }

    #[test]
    fn read_frame_rejects_missing_version_field() {
        let raw =
            b"{\"kind\":\"exec_request\",\"payload\":{\"program\":\"/x\",\"args\":[]}}\n".to_vec();
        let mut cursor = Cursor::new(raw);
        let err = read_frame::<_, Envelope<ExecRequest>>(&mut cursor).unwrap_err();
        assert!(
            matches!(err, ProtoError::MalformedPayload(_)),
            "missing version must surface as MalformedPayload; got {err:?}"
        );
    }

    #[test]
    fn read_frame_rejects_wrong_type_version_field() {
        let raw =
            b"{\"version\":\"1\",\"kind\":\"exec_request\",\"payload\":{\"program\":\"/x\",\"args\":[]}}\n"
                .to_vec();
        let mut cursor = Cursor::new(raw);
        let err = read_frame::<_, Envelope<ExecRequest>>(&mut cursor).unwrap_err();
        assert!(
            matches!(err, ProtoError::MalformedPayload(_)),
            "wrong-type version must surface as MalformedPayload; got {err:?}"
        );
    }
}
