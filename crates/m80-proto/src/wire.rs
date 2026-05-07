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

use generated::*;

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
            payload,
        })
    }
}

impl WireEnvelope {
    pub(crate) fn from_raw(raw: RawEnvelope) -> Self {
        Self {
            version: raw.version,
            kind: raw.kind,
            request_id: raw.request_id,
            payload: Some(raw.payload),
        }
    }

    pub(crate) fn into_raw(self) -> Result<RawEnvelope, ProtoError> {
        Ok(RawEnvelope {
            version: self.version,
            kind: self.kind,
            request_id: self.request_id,
            payload: self
                .payload
                .ok_or_else(|| ProtoError::MalformedPayload("missing envelope payload".into()))?,
        })
    }
}

/// Encode a raw envelope into a protobuf frame body.
pub fn encode_raw_envelope(raw: RawEnvelope) -> Result<Vec<u8>, ProtoError> {
    let wire = WireEnvelope::from_raw(raw);
    if wire.version != PROTOCOL_VERSION {
        return Err(ProtoError::IncompatibleVersion {
            expected: PROTOCOL_VERSION,
            got: wire.version,
        });
    }
    Ok(wire.encode_to_vec())
}

/// Decode a protobuf frame body into a raw envelope.
pub fn decode_raw_envelope(bytes: &[u8]) -> Result<RawEnvelope, ProtoError> {
    WireEnvelope::decode(bytes)
        .map_err(|e| ProtoError::MalformedPayload(e.to_string()))?
        .into_raw()
}
