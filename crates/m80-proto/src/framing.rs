//! Length-prefixed protobuf framing.

use std::io::{Read, Write};

use crate::error::ProtoError;
use crate::types::{Envelope, Payload};
use crate::version::{MAX_FRAME_BYTES, PROTOCOL_VERSION};
use crate::wire::{decode_raw_envelope, encode_raw_envelope, RawEnvelope};

const LENGTH_PREFIX_BYTES: usize = 4;

fn check_size(n: usize) -> Result<(), ProtoError> {
    if n > MAX_FRAME_BYTES {
        return Err(ProtoError::OversizedPayload {
            size: n,
            limit: MAX_FRAME_BYTES,
        });
    }
    Ok(())
}

/// Read one typed protobuf frame from `reader`.
pub fn read_frame<R, T>(reader: &mut R) -> Result<Envelope<T>, ProtoError>
where
    R: Read,
    T: Payload,
{
    read_raw_frame(reader)?.decode()
}

/// Read one protobuf frame from `reader` without choosing a payload type.
pub fn read_raw_frame<R>(reader: &mut R) -> Result<RawEnvelope, ProtoError>
where
    R: Read,
{
    let mut prefix = [0u8; LENGTH_PREFIX_BYTES];
    reader
        .read_exact(&mut prefix)
        .map_err(ProtoError::Io)?;
    let size = u32::from_be_bytes(prefix) as usize;
    check_size(size)?;

    let mut body = vec![0u8; size];
    reader.read_exact(&mut body).map_err(ProtoError::Io)?;
    let raw = decode_raw_envelope(&body)?;
    if raw.version != PROTOCOL_VERSION {
        return Err(ProtoError::IncompatibleVersion {
            expected: PROTOCOL_VERSION,
            got: raw.version,
        });
    }
    Ok(raw)
}

/// Write one typed protobuf frame to `writer`.
pub fn write_frame<W, T>(writer: &mut W, envelope: &Envelope<T>) -> Result<(), ProtoError>
where
    W: Write,
    T: Payload,
{
    write_raw_frame(writer, RawEnvelope::from_typed(envelope.clone()))
}

/// Write one protobuf frame to `writer` without choosing a payload type.
pub fn write_raw_frame<W>(writer: &mut W, envelope: RawEnvelope) -> Result<(), ProtoError>
where
    W: Write,
{
    let body = encode_raw_envelope(envelope)?;
    check_size(body.len())?;
    // check_size guarantees body.len() <= MAX_FRAME_BYTES = 4 MiB, well within u32::MAX.
    let size = body.len() as u32;
    writer.write_all(&size.to_be_bytes())?;
    writer.write_all(&body)?;
    Ok(())
}
