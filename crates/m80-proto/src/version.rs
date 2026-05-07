//! Protocol version and frame-size cap.

/// Wire-protocol version. m80 v0.1 is hard-pinned to this exact value;
/// mismatch fails closed.
///
/// There is exactly one live protocol version at any time. When a protocol
/// change ships, `PROTOCOL_VERSION` is bumped and all hosts and guests must
/// run the new version atomically. Backward-compatible range checks
/// (`MIN..=MAX`), dual-version dispatch paths, and host-side translation shims
/// are explicitly forbidden — fix the deploy pipeline, not the protocol.
pub const PROTOCOL_VERSION: u32 = 3;

/// Maximum size of a single protobuf frame body in bytes.
///
/// The check is strict `>`: a frame whose body length equals exactly
/// `MAX_FRAME_BYTES` still passes; `MAX_FRAME_BYTES + 1` is rejected with
/// [`crate::ProtoError::OversizedPayload`]. The cap is a per-frame allocation guard,
/// not an application-level transfer cap; bulk data must use chunk payloads.
pub const MAX_FRAME_BYTES: usize = 4 * 1024 * 1024;
