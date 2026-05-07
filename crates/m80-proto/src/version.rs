//! Protocol version, frame-size cap, and the version-negotiation helper.

use crate::error::ProtoError;

/// Wire-protocol version. m80 v0.1 is hard-pinned to `1`; mismatch fails closed.
///
/// There is exactly one live protocol version at any time. When a protocol
/// change ships, `PROTOCOL_VERSION` is bumped and all hosts and guests must
/// run the new version atomically. Backward-compatible range checks
/// (`MIN..=MAX`), dual-version dispatch paths, and host-side translation shims
/// are explicitly forbidden — fix the deploy pipeline, not the protocol.
pub const PROTOCOL_VERSION: u32 = 1;

/// Maximum size of a single protobuf frame body in bytes.
///
/// The check is strict `>`: a frame whose body length equals exactly
/// `MAX_FRAME_BYTES` still passes; `MAX_FRAME_BYTES + 1` is rejected with
/// [`ProtoError::OversizedPayload`]. The cap is a per-frame allocation guard,
/// not an application-level transfer cap; bulk data must use chunk payloads.
pub const MAX_FRAME_BYTES: usize = 4 * 1024 * 1024;

/// Validate that `remote` is the single live protocol version.
///
/// Returns `Ok(())` iff `remote == PROTOCOL_VERSION`. There is no range-based
/// fallback.
pub fn negotiate_version(remote: u32) -> Result<(), ProtoError> {
    if remote == PROTOCOL_VERSION {
        Ok(())
    } else {
        Err(ProtoError::IncompatibleVersion {
            expected: PROTOCOL_VERSION,
            got: remote,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negotiate_version_ok_for_matching() {
        assert!(negotiate_version(PROTOCOL_VERSION).is_ok());
    }

    #[test]
    fn negotiate_version_err_for_newer() {
        let err = negotiate_version(PROTOCOL_VERSION + 1).unwrap_err();
        assert!(matches!(
            err,
            ProtoError::IncompatibleVersion { expected, got }
                if expected == PROTOCOL_VERSION && got == PROTOCOL_VERSION + 1
        ));
    }

    #[test]
    fn negotiate_version_err_for_older() {
        let err = negotiate_version(0).unwrap_err();
        assert!(matches!(
            err,
            ProtoError::IncompatibleVersion { expected, got }
                if expected == PROTOCOL_VERSION && got == 0
        ));
    }
}
