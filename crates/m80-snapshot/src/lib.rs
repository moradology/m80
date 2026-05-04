//! Snapshot manifest schemas and persistence-path layout. Schemas active in
//! v0.1; capture/restore return [`SnapshotError::Deferred`] until v0.2.
//! See `README.md` for the contract. Behavior captures: bead epic `m80-0tf`.

#![deny(missing_docs)]

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Manifest schema version. m80 v0.1 ships `1`; future versions are new code,
/// not migrations.
pub const SCHEMA_VERSION: u32 = 1;

/// File name for the snapshot manifest in the snapshot directory.
pub const SNAPSHOT_MANIFEST_FILE: &str = "snapshot-manifest.json";

/// File name for the restore metadata in the snapshot directory.
pub const RESTORE_METADATA_FILE: &str = "restore-metadata.json";

/// Manifest persisted alongside a snapshot at
/// `<store-root>/<workspace_id>/<run_id>/<unix_ms>-<sha>/snapshot-manifest.json`.
///
/// Field declaration order is alphabetical so JSON serialization is stable
/// without a canonicalization pass.
///
/// **v0.1 status:** the schema is stable and serializes to the canonical
/// JSON shape v0.2 will read. The execution lane (capture/restore) returns
/// [`SnapshotError::Deferred`] in v0.1; only the schema + path helpers are
/// active. A snapshot persisted today by a v0.2 build will be readable; the
/// inverse is not guaranteed (v0.2 may add fields).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotManifest {
    /// sha256 over the artifact set in declared order.
    pub artifact_set_sha256: String,
    /// The five-element artifact set in declared order. Optional artifacts
    /// (diagnostics, metrics) may be appended after the required five.
    pub artifacts: Vec<Artifact>,
    /// Unix epoch milliseconds at capture time.
    pub created_at_unix_ms: u64,
    /// Optional diagnostics bundle artifact.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagnostics_bundle: Option<Artifact>,
    /// Firecracker version pin.
    pub expected_firecracker_version: String,
    /// Optional metrics snapshot artifact.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metrics_snapshot: Option<Artifact>,
    /// Schema version. v0.1 = `1`.
    pub schema_version: u32,
    /// Source run identifier (caller-supplied opaque string).
    pub source_run_id: String,
    /// Source VM identifier (caller-supplied opaque string).
    pub source_vm_id: String,
    /// Source workspace identifier (caller-supplied opaque string).
    pub source_workspace_id: String,
}

/// Restore metadata persisted alongside the manifest.
///
/// Field declaration order is alphabetical so JSON serialization is stable
/// without a canonicalization pass.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RestoreMetadata {
    /// Firecracker version the snapshot is pinned to.
    pub expected_firecracker_version: String,
    /// Schema version. v0.1 = `1`.
    pub schema_version: u32,
    /// Path to the snapshot directory.
    pub snapshot_path: PathBuf,
    /// Source run identifier.
    pub source_run_id: String,
    /// Source VM identifier (the restored VM gets a different `vm_id`).
    pub source_vm_id: String,
    /// Source workspace identifier.
    pub source_workspace_id: String,
}

/// One artifact in the snapshot set.
///
/// Field declaration order is alphabetical so JSON serialization is stable
/// without a canonicalization pass.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
    /// Boot identity record.
    BootIdentity,
    /// Memory image.
    Memory,
    /// Runtime rootfs clone.
    RuntimeRootfs,
    /// VM state file produced by Firecracker's `CreateSnapshot`.
    VmState,
    /// Workspace scratch image.
    WorkspaceScratch,
}

/// Partial-deserialize struct used to extract `schema_version` before
/// committing to a full parse. Without this, a v0.2 file stamped
/// `schema_version: 2` plus a new field surfaces as a
/// `Json("unknown field …")` (because the structs carry
/// `#[serde(deny_unknown_fields)]`) instead of `UnsupportedSchemaVersion(2)`.
#[derive(Deserialize)]
struct SchemaVersionProbe {
    schema_version: u32,
}

/// Write `value` to `path` as pretty JSON + trailing newline + mode 0644.
fn write_pretty_json_0644<T: Serialize>(value: &T, path: &Path) -> Result<(), SnapshotError> {
    let mut json = serde_json::to_string_pretty(value).map_err(SnapshotError::Json)?;
    json.push('\n');
    std::fs::write(path, json.as_bytes()).map_err(|source| SnapshotError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o644)).map_err(
            |source| SnapshotError::Io {
                path: path.to_path_buf(),
                source,
            },
        )?;
    }
    Ok(())
}

/// Probe `schema_version` before full parse so a v0.2 file reports
/// `UnsupportedSchemaVersion(2)` instead of leaking the unrelated
/// `Json("unknown field …")` from `deny_unknown_fields`.
fn read_with_schema_probe<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, SnapshotError> {
    let raw = std::fs::read(path).map_err(|source| SnapshotError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let probe: SchemaVersionProbe = serde_json::from_slice(&raw).map_err(SnapshotError::Json)?;
    if probe.schema_version != SCHEMA_VERSION {
        return Err(SnapshotError::UnsupportedSchemaVersion(probe.schema_version));
    }
    serde_json::from_slice(&raw).map_err(SnapshotError::Json)
}

impl SnapshotManifest {
    /// Write to `path` as pretty JSON + trailing newline + mode 0644 (Unix).
    pub fn write(&self, path: &Path) -> Result<(), SnapshotError> {
        write_pretty_json_0644(self, path)
    }

    /// Read and structurally validate a manifest at `path`. Probes
    /// `schema_version` before full parse; sha256s are NOT checked here.
    pub fn read(path: &Path) -> Result<SnapshotManifest, SnapshotError> {
        read_with_schema_probe(path)
    }
}

impl RestoreMetadata {
    /// Write to `path` as pretty JSON + trailing newline + mode 0644 (Unix).
    pub fn write(&self, path: &Path) -> Result<(), SnapshotError> {
        write_pretty_json_0644(self, path)
    }

    /// Read and structurally validate restore metadata at `path`.
    pub fn read(path: &Path) -> Result<RestoreMetadata, SnapshotError> {
        read_with_schema_probe(path)
    }
}

/// Compute the canonical persistence path for a snapshot.
///
/// Format: `<store_root>/<workspace_id>/<run_id>/<created_at_unix_ms>-<artifact_set_sha256>/`.
///
/// Pure path construction; no I/O. Collision detection is a capture-time
/// concern (`SnapshotError::DestinationCollision`); v0.1 doesn't perform
/// capture.
///
/// The `<store_root>` must be a host-local filesystem path. v0.1 ships no
/// remote-store support; adding S3/GCS/generic stores is a v0.2+ epic.
///
/// **Caller responsibility — no path-component sanitization.** Both
/// `workspace_id` and `run_id` are joined with `Path::join` directly, so a
/// value containing `/` or `..` *will* produce out-of-tree paths
/// (e.g., `workspace_id = "../../etc"`). Callers handling untrusted IDs
/// must reject those characters at the API boundary before calling.
/// (The unit test `workspace_id_is_not_sanitised` documents this contract.)
///
/// # Example
///
/// ```ignore
/// let p = persistence_path(
///     std::path::Path::new("/var/snapshots"),
///     "ws-1",
///     "run-42",
///     1_700_000_000_000,
///     "abc123",
/// );
/// assert_eq!(p.to_str(), Some("/var/snapshots/ws-1/run-42/1700000000000-abc123"));
/// ```
pub fn persistence_path(
    store_root: &Path,
    workspace_id: &str,
    run_id: &str,
    created_at_unix_ms: u64,
    artifact_set_sha256: &str,
) -> PathBuf {
    store_root
        .join(workspace_id)
        .join(run_id)
        .join(format!("{created_at_unix_ms}-{artifact_set_sha256}"))
}

/// Compute the canonical sha256 over an artifact set in declared order.
///
/// The caller is responsible for putting artifacts in canonical order before
/// calling this function. Same artifacts in a different order produce a
/// different digest — this is intentional.
///
/// Each artifact is serialized to JSON (field order is alphabetical due to
/// struct declaration order) and the bytes are fed into a single SHA-256
/// accumulator in slice order.
///
/// Note: `path` fields use [`std::path::Path::to_string_lossy`] internally
/// through serde's `PathBuf` serialization, which is platform-specific.
/// Snapshots are not portable across host platforms.
pub fn artifact_set_sha256(artifacts: &[Artifact]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    for artifact in artifacts {
        let bytes = serde_json::to_vec(artifact).expect("Artifact serialization is infallible");
        hasher.update(&bytes);
    }
    hasher.finalize().into()
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
    /// `schema_version` in the file did not match [`SCHEMA_VERSION`].
    #[error("unsupported snapshot schema version: got {0}, expected {SCHEMA_VERSION}")]
    UnsupportedSchemaVersion(u32),
    /// Underlying I/O failure; carries the path so the caller doesn't have
    /// to guess which file failed.
    #[error("i/o on {}: {source}", path.display())]
    Io {
        /// File the I/O was attempted against.
        path: PathBuf,
        /// Underlying I/O error.
        #[source]
        source: io::Error,
    },
    /// JSON encode/decode failure (malformed JSON, missing required field,
    /// or unknown field rejected by `deny_unknown_fields`).
    #[error("json: {0}")]
    Json(serde_json::Error),
}
