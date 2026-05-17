//! Snapshot-template store errors.

use std::io;
use std::path::PathBuf;

use crate::TemplateFingerprint;

/// Errors surfaced by snapshot-template store operations.
#[derive(Debug, thiserror::Error)]
pub enum TemplateStoreError {
    /// Store capacity was zero.
    #[error("snapshot-template store capacity must be greater than zero")]
    CapacityZero,
    /// Store root or internal layout is invalid.
    #[error("invalid snapshot-template store root {}: {reason}", path.display())]
    InvalidStoreRoot {
        /// Rejected path.
        path: PathBuf,
        /// Finite rejection reason.
        reason: &'static str,
    },
    /// Typed input validation failed.
    #[error("invalid snapshot-template value for {field}: {reason}")]
    InvalidValue {
        /// Field whose value was rejected.
        field: &'static str,
        /// Finite rejection reason.
        reason: &'static str,
    },
    /// The requested template does not exist.
    #[error("snapshot template {fingerprint} is missing")]
    TemplateMissing {
        /// Missing fingerprint.
        fingerprint: TemplateFingerprint,
    },
    /// A template body already exists at the destination fingerprint.
    #[error("snapshot template {fingerprint} already exists")]
    TemplateExists {
        /// Existing fingerprint.
        fingerprint: TemplateFingerprint,
    },
    /// The requested template is currently pinned in this process.
    #[error("snapshot template {fingerprint} is pinned")]
    TemplatePinned {
        /// Pinned fingerprint.
        fingerprint: TemplateFingerprint,
    },
    /// A committed template's stored identity does not match live inputs.
    #[error("snapshot-template fingerprint mismatch: stored {stored}, live {live}")]
    FingerprintMismatch {
        /// Stored or path-derived fingerprint.
        stored: TemplateFingerprint,
        /// Fingerprint computed from live inputs.
        live: TemplateFingerprint,
    },
    /// A body file required before commit is missing.
    #[error("snapshot-template body is incomplete; missing {}", path.display())]
    MissingBody {
        /// Missing body path.
        path: PathBuf,
    },
    /// Eviction could not remove any template because all candidates are pinned.
    #[error("snapshot-template store is over capacity but all candidates are pinned")]
    AllCandidatesPinned,
    /// A filesystem operation failed.
    #[error("snapshot-template I/O at {}: {source}", path.display())]
    Io {
        /// Path involved in the failed operation.
        path: PathBuf,
        /// Underlying I/O error.
        #[source]
        source: io::Error,
    },
    /// `schema_version` in a JSON file did not match the active schema.
    #[error("unsupported snapshot-template schema version: got {got}, expected {expected}")]
    UnsupportedSchemaVersion {
        /// Version found in the file.
        got: u32,
        /// Version supported by this crate.
        expected: u32,
    },
    /// JSON encode/decode failed.
    #[error("snapshot-template JSON at {}: {source}", path.display())]
    Json {
        /// File being encoded or decoded.
        path: PathBuf,
        /// serde_json error.
        #[source]
        source: serde_json::Error,
    },
}

pub(crate) fn wrap_io(path: &std::path::Path) -> impl Fn(io::Error) -> TemplateStoreError + '_ {
    |source| TemplateStoreError::Io {
        path: path.to_path_buf(),
        source,
    }
}
