//! Protobuf wire messages and conversions for the active host/guest protocol.

mod conversions;

use prost::Message;

use crate::error::ProtoError;
use crate::types::Payload;
use crate::version::PROTOCOL_VERSION;

#[doc(hidden)]
pub mod generated {
    #![allow(missing_docs)]

    include!(concat!(env!("OUT_DIR"), "/m80.wire.rs"));
}

use generated::{WireCancelAck, WireCancelRequest, WireDirEntry, WireEnvVar, WireEnvelope, WireExecExit, WireExecRequest, WireExecResponse, WireExecStreamChunk, WireExecTiming, WireFileListRequest, WireFileListResponse, WireFileMkdirRequest, WireFileMkdirResponse, WireFileReadChunk, WireFileReadRequest, WireFileReadResponse, WireFileRemoveRequest, WireFileRemoveResponse, WireFileStat, WireFileStatRequest, WireFileStatResponse, WireFileWriteBeginRequest, WireFileWriteBeginResponse, WireFileWriteChunkRequest, WireFileWriteChunkResponse, WireFileWriteCommitRequest, WireFileWriteCommitResponse, WireFileWriteRequest, WireFileWriteResponse, WireGuestCpuMetrics, WireGuestMemMetrics, WireHandshakeMessage, WireMetricsRequest, WireMetricsResponse, WirePingRequest, WirePongResponse, WirePtyBytes, WirePtyControl, WirePtyExit, WirePtyRequest, WirePtyResize, WirePtySize, WireShutdownRequest, WireShutdownResponse};

/// Active protobuf payload variants.
pub use generated::wire_envelope::Payload as WirePayload;
/// Active protobuf PTY control event variants.
pub(crate) use generated::wire_pty_control::Event as WirePtyControlEvent;

/// Protobuf envelope decoded without committing to a concrete payload type.
#[derive(Debug, Clone, PartialEq)]
pub struct RawEnvelope {
    /// Protocol version.
    pub version: u32,
    /// Payload kind discriminator.
    pub kind: String,
    /// Opaque request identifier.
    pub request_id: Option<String>,
    /// Optional call-wide wall-clock budget in milliseconds.
    pub max_duration_ms: Option<u64>,
    /// Protobuf payload variant.
    pub payload: WirePayload,
}

impl RawEnvelope {
    /// Construct a raw envelope from a typed payload.
    pub fn from_typed<T: Payload>(envelope: crate::types::Envelope<T>) -> Self {
        Self {
            version: envelope.version,
            kind: envelope.kind,
            request_id: envelope.request_id,
            max_duration_ms: envelope.max_duration_ms,
            payload: envelope.payload.into_wire(),
        }
    }

    /// Decode the raw payload into the requested m80 payload type.
    pub fn decode<T: Payload>(self) -> Result<crate::types::Envelope<T>, ProtoError> {
        if self.kind != T::KIND {
            return Err(ProtoError::MalformedPayload(format!(
                "payload kind mismatch: envelope kind {:?}, decoded kind {:?}",
                self.kind,
                T::KIND
            )));
        }
        let payload = T::from_wire(self.payload)?;
        Ok(crate::types::Envelope {
            version: self.version,
            kind: self.kind,
            request_id: self.request_id,
            max_duration_ms: self.max_duration_ms,
            payload,
        })
    }
}

/// Encode a raw envelope into a protobuf frame body.
pub fn encode_raw_envelope(raw: RawEnvelope) -> Result<Vec<u8>, ProtoError> {
    if raw.version != PROTOCOL_VERSION {
        return Err(ProtoError::IncompatibleVersion {
            expected: PROTOCOL_VERSION,
            got: raw.version,
        });
    }
    let wire = WireEnvelope {
        version: raw.version,
        kind: raw.kind,
        request_id: raw.request_id,
        max_duration_ms: raw.max_duration_ms,
        payload: Some(raw.payload),
    };
    Ok(wire.encode_to_vec())
}

/// Decode a protobuf frame body into a raw envelope.
pub(crate) fn decode_raw_envelope(bytes: &[u8]) -> Result<RawEnvelope, ProtoError> {
    reject_unknown_envelope_fields(bytes)?;
    let wire = WireEnvelope::decode(bytes)
        .map_err(|e| ProtoError::MalformedPayload(e.to_string()))?;
    Ok(RawEnvelope {
        version: wire.version,
        kind: wire.kind,
        request_id: wire.request_id,
        max_duration_ms: wire.max_duration_ms,
        payload: wire
            .payload
            .ok_or_else(|| ProtoError::MalformedPayload("missing envelope payload".into()))?,
    })
}

fn reject_unknown_envelope_fields(bytes: &[u8]) -> Result<(), ProtoError> {
    let mut offset = 0;
    while offset < bytes.len() {
        let key = read_varint(bytes, &mut offset)?;
        let field = key >> 3;
        let wire_type = key & 0x07;
        if !known_envelope_field(field) {
            return Err(ProtoError::MalformedPayload(format!(
                "unknown envelope field: {field}"
            )));
        }
        skip_field(bytes, &mut offset, wire_type)?;
    }
    Ok(())
}

fn known_envelope_field(field: u64) -> bool {
    matches!(field, 1..=4 | 10..=52)
}

fn read_varint(bytes: &[u8], offset: &mut usize) -> Result<u64, ProtoError> {
    let mut value = 0u64;
    for shift in (0..64).step_by(7) {
        let Some(byte) = bytes.get(*offset).copied() else {
            return Err(ProtoError::MalformedPayload(
                "truncated protobuf varint".into(),
            ));
        };
        *offset += 1;
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Ok(value);
        }
    }
    Err(ProtoError::MalformedPayload(
        "protobuf varint exceeds 64 bits".into(),
    ))
}

fn skip_field(bytes: &[u8], offset: &mut usize, wire_type: u64) -> Result<(), ProtoError> {
    match wire_type {
        0 => {
            let _ = read_varint(bytes, offset)?;
        }
        1 => skip_bytes(bytes, offset, 8)?,
        2 => {
            let len = read_varint(bytes, offset)?;
            let len = usize::try_from(len)
                .map_err(|_| ProtoError::MalformedPayload("field length too large".into()))?;
            skip_bytes(bytes, offset, len)?;
        }
        5 => skip_bytes(bytes, offset, 4)?,
        other => {
            return Err(ProtoError::MalformedPayload(format!(
                "unsupported protobuf wire type: {other}"
            )));
        }
    }
    Ok(())
}

fn skip_bytes(bytes: &[u8], offset: &mut usize, len: usize) -> Result<(), ProtoError> {
    let end = offset
        .checked_add(len)
        .ok_or_else(|| ProtoError::MalformedPayload("field length overflows usize".into()))?;
    if end > bytes.len() {
        return Err(ProtoError::MalformedPayload(
            "truncated protobuf field".into(),
        ));
    }
    *offset = end;
    Ok(())
}
