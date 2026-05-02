//! Wire envelope and NDJSON framing for m80 host↔guest communication.
//!
//! Both the host (`m80-firecracker`) and in-VM daemon (`m80-guestd`) depend on
//! this crate. Every value that crosses the vsock boundary is wrapped in an
//! [`Envelope`] and serialized as a single NDJSON record.
//!
//! Peers exchange a [`HandshakeMessage`] on every fresh connection before
//! processing any application payload. [`negotiate_version`] validates the
//! remote version. `Vec<u8>` fields encode as base64 (~33% inflation), so the
//! practical raw-output budget per [`MAX_FRAME_BYTES`]-capped frame is around
//! 2.9 MiB. The `kind` discriminator on [`Envelope`] reserves the extension
//! point for v0.2+ payload types without bumping [`PROTOCOL_VERSION`].

#![deny(missing_docs)]

mod error;
mod framing;
mod types;
mod version;

pub use error::ProtoError;
pub use framing::{read_frame, write_frame};
pub use types::{
    Envelope, ExecRequest, ExecResponse, ExecStatus, ExecTiming, HandshakeMessage,
    PAYLOAD_KIND_EXEC_REQUEST, PAYLOAD_KIND_EXEC_RESPONSE, Payload,
};
pub use version::{MAX_FRAME_BYTES, PROTOCOL_VERSION, negotiate_version};

/// Default vsock port the in-VM `m80-guestd` daemon listens on.
///
/// Both the host (via `m80-vsock`/`m80-firecracker`) and the guest daemon
/// must agree on this value; it is the single source of truth for the
/// host↔guest port convention.
pub const GUEST_PORT_DEFAULT: u32 = 9001;

/// Default ready-marker that `m80-guestd` prints to the serial console once
/// it is listening on vsock.
///
/// The host watches the console file for this exact string (matched as a full
/// line). Both peers must agree on it; defined here so neither side
/// independently hard-codes the string.
pub const READY_MARKER_DEFAULT: &str = "GUESTD_READY";
