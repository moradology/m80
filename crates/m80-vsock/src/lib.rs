//! Host-side bridge between Firecracker UDS and the guest's vsock CID/port.
//!
//! See `README.md` for the black-box contract.
//! Behavior captures: bead epic `m80-xjg` (`br show m80-xjg`).
//!
//! # Type-pinning pass
//!
//! Public surface is declared here; bodies are `todo!()`. Implementation lands
//! in a later wave.

#![deny(missing_docs)]

use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Serialize;
use serde::de::DeserializeOwned;

use m80_proto::{Envelope, ProtoError};

/// The conventional ready-marker the in-VM daemon writes to the serial console
/// once it's listening on vsock.
pub const READY_MARKER_DEFAULT: &str = "GUESTD_READY";

/// The conventional vsock port the in-VM daemon listens on.
pub const GUEST_PORT_DEFAULT: u32 = 9001;

/// Derive the vsock CID a VM should be assigned from its `vm_id`.
///
/// Pure: no I/O, no allocation table. Two VMs with the same `vm_id` get the
/// same CID; different `vm_id`s get distinct CIDs (modulo the 32-bit space).
pub fn cid_for_vm_id(_vm_id: &str) -> u32 {
    todo!()
}

/// One open connection to the in-VM daemon. Created by [`Channel::open`];
/// dropped when the request/response cycle is done.
#[derive(Debug)]
pub struct Channel {
    _host_uds: PathBuf,
}

impl Channel {
    /// Watch `console` for `ready_marker`, then connect to `host_uds` and
    /// hand-shake to the guest's vsock `guest_port`. `timeout` bounds the
    /// ready-watch step.
    ///
    /// The default port is [`GUEST_PORT_DEFAULT`] and the default marker is
    /// [`READY_MARKER_DEFAULT`]; callers typically pass those directly.
    pub fn open(
        _host_uds: &Path,
        _guest_port: u32,
        _ready_marker: &str,
        _console: &Path,
        _timeout: Duration,
    ) -> Result<Self, VsockError> {
        todo!()
    }

    /// Send one [`Envelope`] over the channel.
    pub fn send<T: Serialize>(&mut self, _envelope: &Envelope<T>) -> Result<(), VsockError> {
        todo!()
    }

    /// Receive one [`Envelope`] from the channel.
    pub fn recv<U: DeserializeOwned>(&mut self) -> Result<Envelope<U>, VsockError> {
        todo!()
    }

    /// Close the connection and remove the host-side UDS. Idempotent.
    pub fn close(self) -> Result<(), VsockError> {
        todo!()
    }
}

/// Errors surfaced by [`Channel`] operations.
#[derive(Debug, thiserror::Error)]
pub enum VsockError {
    /// Ready marker was not seen on the serial console within the timeout.
    #[error("guestd ready marker not observed before timeout")]
    NotReady,
    /// Connect to the Firecracker UDS or vsock-bridge handshake failed.
    #[error("vsock connect failed (errno={errno})")]
    ConnectFailed {
        /// libc errno reported by the connect call.
        errno: i32,
    },
    /// The Firecracker UDS-to-vsock handshake was malformed.
    #[error("vsock handshake failed")]
    HandshakeFailed,
    /// Tried to open a second [`Channel`] on a VM that already had one.
    #[error("vsock channel already in use for this VM")]
    ChannelInUse,
    /// Underlying I/O failure.
    #[error("i/o: {0}")]
    Io(#[from] io::Error),
    /// Frame-level protocol error.
    #[error("proto: {0}")]
    Proto(#[from] ProtoError),
}
