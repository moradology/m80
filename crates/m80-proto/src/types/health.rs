//! Guest health probe payloads.

/// Wire `kind` value for an envelope carrying [`PingRequest`].
pub const PAYLOAD_KIND_PING_REQUEST: &str = "ping_request";

/// Wire `kind` value for an envelope carrying [`PongResponse`].
pub const PAYLOAD_KIND_PONG_RESPONSE: &str = "pong_response";

/// Health probe request payload — host → guest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PingRequest {}

/// Health probe response payload — guest → host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PongResponse {
    /// Guest wall-clock timestamp in Unix milliseconds when guestd handled
    /// the ping.
    pub guest_unix_ms: u64,
}
