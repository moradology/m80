//! Length-prefixed protobuf framing.

use std::io::{self, Read, Write};

use crate::error::ProtoError;
use crate::types::{Envelope, HandshakeMessage, Payload};
use crate::version::{MAX_FRAME_BYTES, PROTOCOL_VERSION};
use crate::wire::{decode_raw_envelope, encode_raw_envelope, RawEnvelope};

const LENGTH_PREFIX_BYTES: usize = 4;

/// A value that can be encoded as one m80 protobuf frame.
pub trait Frame: Sized {
    /// Convert this value into a raw envelope for writing.
    fn to_raw_frame(&self) -> Result<RawEnvelope, ProtoError>;
    /// Decode this value from a raw envelope after reading.
    fn from_raw_frame(raw: RawEnvelope) -> Result<Self, ProtoError>;
}

impl<T> Frame for Envelope<T>
where
    T: Payload + Clone,
{
    fn to_raw_frame(&self) -> Result<RawEnvelope, ProtoError> {
        Ok(RawEnvelope::from_typed(self.clone()))
    }

    fn from_raw_frame(raw: RawEnvelope) -> Result<Self, ProtoError> {
        raw.decode()
    }
}

impl Frame for HandshakeMessage {
    fn to_raw_frame(&self) -> Result<RawEnvelope, ProtoError> {
        Ok(RawEnvelope::from_typed(Envelope::new(self.clone())))
    }

    fn from_raw_frame(raw: RawEnvelope) -> Result<Self, ProtoError> {
        Ok(raw.decode::<HandshakeMessage>()?.payload)
    }
}

/// Read one typed protobuf frame from `reader`.
pub fn read_frame<R, T>(reader: &mut R) -> Result<T, ProtoError>
where
    R: Read,
    T: Frame,
{
    T::from_raw_frame(read_raw_frame(reader)?)
}

/// Read one protobuf frame from `reader` without choosing a payload type.
pub fn read_raw_frame<R>(reader: &mut R) -> Result<RawEnvelope, ProtoError>
where
    R: Read,
{
    let mut prefix = [0u8; LENGTH_PREFIX_BYTES];
    reader
        .read_exact(&mut prefix)
        .map_err(map_read_exact_error)?;
    let size = u32::from_be_bytes(prefix) as usize;
    if size > MAX_FRAME_BYTES {
        return Err(ProtoError::OversizedPayload {
            size,
            limit: MAX_FRAME_BYTES,
        });
    }

    let mut body = vec![0u8; size];
    reader.read_exact(&mut body).map_err(map_read_exact_error)?;
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
pub fn write_frame<W, T>(writer: &mut W, value: &T) -> Result<(), ProtoError>
where
    W: Write,
    T: Frame,
{
    write_raw_frame(writer, value.to_raw_frame()?)
}

/// Write one protobuf frame to `writer` without choosing a payload type.
pub fn write_raw_frame<W>(writer: &mut W, envelope: RawEnvelope) -> Result<(), ProtoError>
where
    W: Write,
{
    let body = encode_raw_envelope(envelope)?;
    if body.len() > MAX_FRAME_BYTES {
        return Err(ProtoError::OversizedPayload {
            size: body.len(),
            limit: MAX_FRAME_BYTES,
        });
    }
    let size = u32::try_from(body.len())
        .map_err(|_| ProtoError::EncodeFailed("frame length does not fit in u32".into()))?;
    writer.write_all(&size.to_be_bytes())?;
    writer.write_all(&body)?;
    Ok(())
}

fn map_read_exact_error(err: io::Error) -> ProtoError {
    if err.kind() == io::ErrorKind::UnexpectedEof {
        ProtoError::Io(io::Error::from(io::ErrorKind::UnexpectedEof))
    } else {
        ProtoError::Io(err)
    }
}
