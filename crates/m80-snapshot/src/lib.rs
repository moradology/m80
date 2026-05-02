//! Snapshot manifest schemas and persistence path layout.
//! Schemas active in v0.1; capture/restore execution deferred to v0.2.
//!
//! See `README.md` for the black-box contract.
//! Behavior captures: bead epic `m80-0tf` (`br show m80-0tf`).
//!
//! # Type-pinning pass
//!
//! Schemas + path helpers are pinned. `capture` and `restore` are present in
//! v0.1 but return [`SnapshotError::Deferred`].

#![deny(missing_docs)]

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Manifest persisted alongside a snapshot at
/// `<store-root>/<workspace_id>/<run_id>/<unix_ms>-<sha>/snapshot-manifest.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotManifest {
    /// Schema version. v0.1 = `1`.
    pub schema_version: u32,
    /// Firecracker version pin.
    pub expected_firecracker_version: String,
    /// Source workspace identifier (caller-supplied opaque string).
    pub source_workspace_id: String,
    /// Source run identifier (caller-supplied opaque string).
    pub source_run_id: String,
    /// Source VM identifier (caller-supplied opaque string).
    pub source_vm_id: String,
    /// Unix epoch milliseconds at capture time.
    pub created_at_unix_ms: u64,
    /// The five-element artifact set in declared order.
    pub artifacts: Vec<Artifact>,
    /// Optional diagnostics bundle artifact.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagnostics_bundle: Option<Artifact>,
    /// Optional metrics snapshot artifact.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metrics_snapshot: Option<Artifact>,
    /// sha256 over the artifact set in declared order.
    pub artifact_set_sha256: String,
}

/// Restore metadata persisted alongside the manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RestoreMetadata {
    /// Schema version. v0.1 = `1`.
    pub schema_version: u32,
    /// Source workspace identifier.
    pub source_workspace_id: String,
    /// Source run identifier.
    pub source_run_id: String,
    /// Source VM identifier (the restored VM gets a different `vm_id`).
    pub source_vm_id: String,
    /// Path to the snapshot directory.
    pub snapshot_path: PathBuf,
    /// Firecracker version the snapshot is pinned to.
    pub expected_firecracker_version: String,
}

/// One artifact in the snapshot set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Artifact {
    /// Kind of artifact.
    pub kind: ArtifactKind,
    /// Path on the host (or jail) at capture time.
    pub path: PathBuf,
    /// sha256 hex digest of the artifact bytes.
    pub sha256: String,
    /// Size in bytes.
    pub size: u64,
}

/// The five required artifact kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    /// VM state file produced by Firecracker's `CreateSnapshot`.
    VmState,
    /// Memory image.
    Memory,
    /// Runtime rootfs clone.
    RuntimeRootfs,
    /// Workspace scratch image.
    WorkspaceScratch,
    /// Boot identity record.
    BootIdentity,
}

/// Compute the canonical persistence path for a snapshot.
///
/// Format: `<store_root>/<workspace_id>/<run_id>/<created_at_unix_ms>-<artifact_set_sha256>/`.
pub fn persistence_path(
    _store_root: &Path,
    _workspace_id: &str,
    _run_id: &str,
    _created_at_unix_ms: u64,
    _artifact_set_sha256: &str,
) -> PathBuf {
    todo!()
}

/// Compute the canonical sha256 over an artifact set in declared order.
pub fn artifact_set_sha256(_artifacts: &[Artifact]) -> [u8; 32] {
    todo!()
}

/// Capture a snapshot. **Returns [`SnapshotError::Deferred`] in v0.1.**
pub fn capture() -> Result<SnapshotManifest, SnapshotError> {
    Err(SnapshotError::Deferred)
}

/// Restore from a snapshot. **Returns [`SnapshotError::Deferred`] in v0.1.**
pub fn restore() -> Result<(), SnapshotError> {
    Err(SnapshotError::Deferred)
}

/// Errors surfaced by snapshot operations.
#[derive(Debug, thiserror::Error)]
pub enum SnapshotError {
    /// Capture/restore execution lane is deferred to v0.2.
    #[error("snapshot execution is deferred to v0.2")]
    Deferred,
    /// The persistence destination already exists.
    #[error("snapshot destination already exists")]
    DestinationCollision,
    /// Firecracker version mismatch on restore.
    #[error("firecracker version mismatch on restore: expected {expected}, got {actual}")]
    FirecrackerVersionMismatch {
        /// Pinned version.
        expected: String,
        /// Reported version.
        actual: String,
    },
    /// A recomputed sha256 did not match the recorded value.
    #[error("sha256 mismatch on {artifact}: expected {expected}, got {actual}")]
    Sha256Mismatch {
        /// Artifact whose hash failed.
        artifact: String,
        /// Recorded hex digest.
        expected: String,
        /// Recomputed hex digest.
        actual: String,
    },
    /// Underlying I/O failure.
    #[error("i/o: {0}")]
    Io(#[from] io::Error),
    /// JSON encode/decode failure.
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}
