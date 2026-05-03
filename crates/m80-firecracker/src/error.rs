//! [`FcError`] — top-level error sum for `m80-firecracker`.

use std::io;

use m80_firecracker_client::ClientError;
use m80_image_manifest::ManifestError;
use m80_jailer::JailerError;
use m80_net_outbound::NetError;
use m80_preflight::PreflightError;
use m80_storage::StorageError;
use m80_vsock::VsockError;

/// Top-level error for `m80-firecracker`. Each variant tells the caller
/// which phase failed; the inner cause carries phase-specific detail.
#[derive(Debug, thiserror::Error)]
pub enum FcError {
    /// Preflight check failed.
    #[error("preflight: {0}")]
    Preflight(#[from] PreflightError),
    /// Manifest read/validate failed.
    #[error("manifest: {0}")]
    Manifest(#[from] ManifestError),
    /// Storage operation failed.
    #[error("storage: {0}")]
    Storage(#[from] StorageError),
    /// Jailer materialization or recovery failed.
    #[error("jailer: {0}")]
    Jailer(#[from] JailerError),
    /// Network realization or cleanup failed.
    #[error("network: {0}")]
    Network(#[from] NetError),
    /// Firecracker REST API call failed.
    #[error("client: {0}")]
    Client(#[from] ClientError),
    /// Vsock channel operation failed. Wire-protocol errors arrive here as
    /// `VsockError::Proto(...)` since vsock is the only transport that
    /// speaks `m80-proto` envelopes in this crate; there is no separate
    /// `Proto` variant.
    #[error("vsock: {0}")]
    Vsock(#[from] VsockError),
    /// Admission was refused (semaphore at limit; no permit available).
    #[error("admission refused: {limit} concurrent VMs already running")]
    AdmissionRefused {
        /// Configured admission limit.
        limit: u32,
    },
    /// The lifecycle state machine was in an unexpected state.
    #[error("invalid lifecycle state: expected {expected}, got {actual}")]
    InvalidState {
        /// State the operation expected.
        expected: &'static str,
        /// State the sandbox was actually in.
        actual: &'static str,
    },
    /// Underlying I/O failure.
    #[error("i/o: {0}")]
    Io(#[from] io::Error),
    /// Configuration loading or merging failure.
    #[error("config: {0}")]
    Config(String),
}
