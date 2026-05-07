//! `ProtoError` — typed errors surfaced by framing helpers.

use std::io;

/// Errors surfaced by the framing helpers.
#[derive(Debug, thiserror::Error)]
pub enum ProtoError {
    /// The frame's `version` field did not match `PROTOCOL_VERSION`.
    #[error("protocol version mismatch: expected {expected}, got {got}")]
    IncompatibleVersion {
        /// The version this binary expects (always `PROTOCOL_VERSION`).
        expected: u32,
        /// The version observed on the wire.
        got: u32,
    },
    /// The payload could not be parsed. Connection handlers must drop the
    /// connection on this error; the stream is unrecoverable after a parse
    /// failure.
    #[error("malformed payload: {0}")]
    MalformedPayload(String),
    /// Protobuf encoding failed while building a frame.
    #[error("encode failed: {0}")]
    EncodeFailed(String),
    /// A frame exceeded [`MAX_FRAME_BYTES`].
    #[error("oversized payload: {size} bytes exceeds limit of {limit}")]
    OversizedPayload {
        /// Observed frame size in bytes.
        size: usize,
        /// Configured limit (always [`MAX_FRAME_BYTES`] in v0.1).
        limit: usize,
    },
    /// Underlying I/O failure.
    #[error("i/o: {0}")]
    Io(#[from] io::Error),
}
