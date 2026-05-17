//! Snapshot manifest schemas, persistence-path layout, and capture/restore
//! primitives for Firecracker microVM snapshots.
//!
//! See `README.md` for the full contract.
//! Behavior captures: bead epic `m80-0tf`; capture/restore: `m80-rrp.3.13`.

#![deny(missing_docs)]

use std::io;
use std::path::Path;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sha2::{Digest, Sha256};

// Re-export the client types callers need to construct requests.
use m80_firecracker_client::{
    Client as FirecrackerClient, CreateSnapshotConfig, LoadSnapshotConfig, MemBackendConfig,
    MemBackendType, SnapshotType as ClientSnapshotType, VmState,
};

/// Manifest schema version. m80 v0.1 ships `1`; future versions are new code,
/// not migrations.
pub const SCHEMA_VERSION: u32 = 1;

/// File name for the snapshot manifest in the snapshot directory.
pub const SNAPSHOT_MANIFEST_FILE: &str = "snapshot-manifest.json";

/// File name for the restore metadata in the snapshot directory.
#[cfg(test)]
pub(crate) const RESTORE_METADATA_FILE: &str = "restore-metadata.json";

// ---------------------------------------------------------------------------
// Schema types
// ---------------------------------------------------------------------------

/// Manifest persisted alongside a snapshot at
/// `<store-root>/<workspace_id>/<run_id>/<unix_ms>-<sha>/snapshot-manifest.json`.
///
/// Field declaration order is alphabetical so JSON serialization is stable
/// without a canonicalization pass.
///
/// The schema is stable and serializes to the canonical JSON shape read by
/// the active capture/restore path. Future schema versions are new code, not
/// tolerant migrations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotManifest {
    /// sha256 over the artifact set in declared order.
    pub artifact_set_sha256: String,
    /// Ordered artifact set: memory image then VM state file.
    pub artifacts: Vec<Artifact>,
    /// Unix epoch milliseconds at capture time.
    pub created_at_unix_ms: u64,
    /// Firecracker version pin.
    pub expected_firecracker_version: String,
    /// Schema version. v0.1 = `1`.
    pub schema_version: u32,
}

/// Restore metadata persisted alongside the manifest.
///
/// Field declaration order is alphabetical so JSON serialization is stable
/// without a canonicalization pass.
#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RestoreMetadata {
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

/// Required artifact kinds for the active Firecracker snapshot pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub enum ArtifactKind {
    /// Memory image.
    Memory,
    /// VM state file produced by Firecracker's `CreateSnapshot`.
    VmState,
}

// ---------------------------------------------------------------------------
// Capture/restore types
// ---------------------------------------------------------------------------

/// The two on-disk files that Firecracker writes at snapshot creation time.
///
/// Both paths must be accessible and writable by the Firecracker process
/// inside the jail. The caller is responsible for ensuring the parent
/// directory exists before calling [`capture`].
#[derive(Debug, Clone)]
pub struct SnapshotPaths {
    /// Path to the microVM state file (vCPU registers, device state).
    pub vm_state: PathBuf,
    /// Path to the guest memory file.
    pub mem: PathBuf,
}

/// Which Firecracker snapshot type to request.
///
/// This stays public because [`CaptureRequest`] is the crate's public wrapper
/// over Firecracker's `CreateSnapshot` API, whose request body includes this
/// choice. `m80-firecracker` currently requests [`SnapshotKind::Full`] only;
/// tests in this crate pin both wire encodings without requiring a full
/// orchestrator path for diff snapshots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotKind {
    /// Full snapshot — all guest memory pages are saved.
    Full,
    /// Diff snapshot — only pages dirtied since the previous snapshot.
    Diff,
}

/// Parameters for [`capture`].
#[derive(Debug)]
pub struct CaptureRequest {
    /// Path to the Firecracker API socket.
    pub api_socket: PathBuf,
    /// Firecracker-visible paths for the snapshot artifact pair.
    pub paths: SnapshotPaths,
    /// Host-readable paths for hashing and manifest persistence.
    pub host_paths: SnapshotPaths,
    /// Firecracker version this snapshot is pinned to.
    pub expected_firecracker_version: String,
    /// Full or Diff snapshot.
    pub kind: SnapshotKind,
}

/// Parameters for [`restore`].
#[derive(Debug)]
pub struct RestoreRequest {
    /// Path to the Firecracker API socket for the **new** (restore-target) process.
    pub api_socket: PathBuf,
    /// Firecracker-visible paths for the snapshot artifact pair.
    pub paths: SnapshotPaths,
    /// Host-readable paths for manifest verification.
    pub host_paths: SnapshotPaths,
    /// Firecracker version expected by the restore environment.
    pub expected_firecracker_version: String,
    /// Path to the vsock UDS file from the **previous** VM that must be
    /// removed before Firecracker can rebind it. Removal is skipped only
    /// if the file is absent (`ENOENT`). Any other error is surfaced.
    pub vsock_uds: PathBuf,
    /// If `true`, issue `PATCH /vm {"state":"Resumed"}` after the load
    /// completes. If `false`, the VM is left in the `Paused` state.
    pub resume: bool,
}

// ---------------------------------------------------------------------------
// capture / restore
// ---------------------------------------------------------------------------

/// Pause the VM and write a snapshot pair to the paths in `req.paths`.
///
/// Steps:
/// 1. PATCH `/vm` to `Paused`.
/// 2. PUT `/snapshot/create` with the configured paths and kind.
/// 3. Hash the host-visible snapshot pair and write `snapshot-manifest.json`.
///
/// The VM is left in the `Paused` state after a successful call. The caller
/// decides whether to resume or kill the Firecracker process.
///
/// # Errors
///
/// Returns [`SnapshotError::Client`] if either REST call fails.
/// Returns a typed manifest or artifact error if the snapshot pair cannot be
/// hashed or the manifest cannot be written after Firecracker reports success.
pub fn capture(req: CaptureRequest) -> Result<(), SnapshotError> {
    let client = FirecrackerClient::new(&req.api_socket).map_err(SnapshotError::Client)?;

    client
        .patch_vm_state(VmState::Paused)
        .map_err(SnapshotError::Client)?;

    let snapshot_type = match req.kind {
        SnapshotKind::Full => Some(ClientSnapshotType::Full),
        SnapshotKind::Diff => Some(ClientSnapshotType::Diff),
    };

    client
        .put_snapshot_create(&CreateSnapshotConfig {
            snapshot_path: req.paths.vm_state,
            mem_file_path: req.paths.mem,
            snapshot_type,
        })
        .map_err(SnapshotError::Client)?;

    write_snapshot_manifest(&req.host_paths, &req.expected_firecracker_version)?;

    Ok(())
}

/// Load a snapshot into a new Firecracker process, optionally resuming it.
///
/// Steps:
/// 1. Read `snapshot-manifest.json`, verify snapshot pair sha256s, and reject
///    Firecracker version mismatch.
/// 2. Remove `req.vsock_uds` if present (`ENOENT` is silently ignored;
///    any other error is returned as [`SnapshotError::VsockUdsUnlink`]).
/// 3. PUT `/snapshot/load` with File-backed memory.
/// 4. If `req.resume`: PATCH `/vm` to `Resumed`.
///
/// # Errors
///
/// Returns [`SnapshotError::VsockUdsUnlink`] if the UDS file cannot be
/// removed for a reason other than `NotFound`.
/// Returns [`SnapshotError::Client`] if a REST call fails.
pub fn restore(req: RestoreRequest) -> Result<(), SnapshotError> {
    restore_inner(req, RestoreVerification::VerifyManifest)
}

/// Load a snapshot whose manifest and artifact identity were already verified
/// by a higher-level immutable store.
///
/// This is intended for content-addressed template bodies that were hashed at
/// commit time and pinned before restore. It still removes stale vsock state
/// and issues the same Firecracker load/resume calls as [`restore`], but it
/// skips the per-restore full artifact hash.
pub fn restore_preverified(req: RestoreRequest) -> Result<(), SnapshotError> {
    restore_inner(req, RestoreVerification::Preverified)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RestoreVerification {
    VerifyManifest,
    Preverified,
}

fn restore_inner(
    req: RestoreRequest,
    verification: RestoreVerification,
) -> Result<(), SnapshotError> {
    if verification == RestoreVerification::VerifyManifest {
        verify_snapshot_manifest(&req.host_paths, &req.expected_firecracker_version)?;
    }

    match std::fs::remove_file(&req.vsock_uds) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => {
            return Err(SnapshotError::VsockUdsUnlink {
                path: req.vsock_uds,
                source: e,
            });
        }
    }

    let client = FirecrackerClient::new(&req.api_socket).map_err(SnapshotError::Client)?;

    client
        .put_snapshot_load(&LoadSnapshotConfig {
            snapshot_path: req.paths.vm_state,
            mem_backend: Some(MemBackendConfig {
                backend_type: MemBackendType::File,
                backend_path: req.paths.mem,
            }),
            mem_file_path: None,
            enable_diff_snapshots: None,
            resume_vm: None,
            vsock_override: None,
        })
        .map_err(SnapshotError::Client)?;

    if req.resume {
        client
            .patch_vm_state(VmState::Resumed)
            .map_err(SnapshotError::Client)?;
    }

    Ok(())
}

/// Hash the host-readable snapshot pair and write `snapshot-manifest.json`
/// beside it.
///
/// This is exposed for tests and for orchestrator paths that need to materialize
/// a manifest around an already-created snapshot pair.
///
/// # Errors
///
/// Returns a typed [`SnapshotError`] if the paths are not one snapshot pair,
/// either artifact cannot be read, or the manifest cannot be written.
pub fn write_snapshot_manifest(
    paths: &SnapshotPaths,
    expected_firecracker_version: &str,
) -> Result<(), SnapshotError> {
    let manifest = build_manifest(paths, expected_firecracker_version)?;
    manifest.write(&manifest_path(paths)?)?;
    Ok(())
}

/// Read `snapshot-manifest.json` beside the host-readable snapshot pair and
/// verify the artifact sha256s and Firecracker version pin.
///
/// # Errors
///
/// Returns a typed [`SnapshotError`] for schema errors, missing/mismatched
/// artifacts, artifact-set mismatch, or Firecracker version mismatch.
pub fn verify_snapshot_manifest(
    paths: &SnapshotPaths,
    expected_firecracker_version: &str,
) -> Result<SnapshotManifest, SnapshotError> {
    let path = manifest_path(paths)?;
    let manifest = SnapshotManifest::read(&path).map_err(SnapshotError::Schema)?;
    if manifest.expected_firecracker_version != expected_firecracker_version {
        return Err(SnapshotError::FirecrackerVersionMismatch {
            expected: expected_firecracker_version.to_owned(),
            recorded: manifest.expected_firecracker_version,
        });
    }

    let expected_artifacts = snapshot_artifacts(paths)?;
    if manifest.artifacts.len() != expected_artifacts.len() {
        return Err(SnapshotError::ManifestArtifactSetInvalid {
            detail: format!(
                "expected {} artifacts, got {}",
                expected_artifacts.len(),
                manifest.artifacts.len()
            ),
        });
    }
    for expected in &expected_artifacts {
        let Some(recorded) = manifest
            .artifacts
            .iter()
            .find(|artifact| artifact.kind == expected.kind)
        else {
            return Err(SnapshotError::ManifestMissingArtifact {
                kind: expected.kind,
            });
        };
        if recorded.path != expected.path
            || recorded.size != expected.size
            || recorded.sha256 != expected.sha256
        {
            return Err(SnapshotError::ArtifactMismatch {
                kind: expected.kind,
                path: expected.path.clone(),
                expected_sha256: recorded.sha256.clone(),
                actual_sha256: expected.sha256.clone(),
            });
        }
    }

    let actual_set_sha256 = hex::encode(artifact_set_sha256(&expected_artifacts));
    if manifest.artifact_set_sha256 != actual_set_sha256 {
        return Err(SnapshotError::ArtifactSetMismatch {
            expected_sha256: manifest.artifact_set_sha256,
            actual_sha256: actual_set_sha256,
        });
    }
    Ok(manifest)
}

fn build_manifest(
    paths: &SnapshotPaths,
    expected_firecracker_version: &str,
) -> Result<SnapshotManifest, SnapshotError> {
    let artifacts = snapshot_artifacts(paths)?;
    Ok(SnapshotManifest {
        artifact_set_sha256: hex::encode(artifact_set_sha256(&artifacts)),
        artifacts,
        created_at_unix_ms: unix_time_ms()?,
        expected_firecracker_version: expected_firecracker_version.to_owned(),
        schema_version: SCHEMA_VERSION,
    })
}

fn snapshot_artifacts(paths: &SnapshotPaths) -> Result<Vec<Artifact>, SnapshotError> {
    ensure_same_snapshot_parent(paths)?;
    Ok(vec![
        artifact_for_path(ArtifactKind::Memory, &paths.mem)?,
        artifact_for_path(ArtifactKind::VmState, &paths.vm_state)?,
    ])
}

fn artifact_for_path(kind: ArtifactKind, path: &Path) -> Result<Artifact, SnapshotError> {
    let bytes = std::fs::read(path).map_err(|source| SnapshotError::ArtifactIo {
        path: path.to_path_buf(),
        source,
    })?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Ok(Artifact {
        kind,
        path: path.to_path_buf(),
        sha256: hex::encode(hasher.finalize()),
        size: bytes.len() as u64,
    })
}

fn manifest_path(paths: &SnapshotPaths) -> Result<PathBuf, SnapshotError> {
    let parent = ensure_same_snapshot_parent(paths)?;
    Ok(parent.join(SNAPSHOT_MANIFEST_FILE))
}

fn ensure_same_snapshot_parent(paths: &SnapshotPaths) -> Result<PathBuf, SnapshotError> {
    let vm_parent = paths
        .vm_state
        .parent()
        .ok_or_else(|| SnapshotError::InvalidSnapshotPaths {
            detail: "vm_state path must have a parent directory".to_owned(),
        })?;
    let mem_parent = paths
        .mem
        .parent()
        .ok_or_else(|| SnapshotError::InvalidSnapshotPaths {
            detail: "mem path must have a parent directory".to_owned(),
        })?;
    if vm_parent != mem_parent {
        return Err(SnapshotError::InvalidSnapshotPaths {
            detail: "vm_state and mem paths must live in the same directory".to_owned(),
        });
    }
    Ok(vm_parent.to_path_buf())
}

fn unix_time_ms() -> Result<u64, SnapshotError> {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|source| SnapshotError::Clock {
            detail: source.to_string(),
        })?;
    Ok(elapsed.as_millis() as u64)
}

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors surfaced by snapshot operations.
#[derive(Debug, thiserror::Error)]
pub enum SnapshotError {
    /// A Firecracker REST call failed.
    #[error("firecracker client: {0}")]
    Client(#[from] m80_firecracker_client::ClientError),
    /// Snapshot manifest schema failed to read or parse.
    #[error("snapshot manifest: {0}")]
    Schema(#[from] SchemaError),
    /// Snapshot artifact file could not be read for hashing.
    #[error("snapshot artifact I/O at {}: {source}", path.display())]
    ArtifactIo {
        /// Path that could not be read.
        path: PathBuf,
        /// Underlying I/O error.
        #[source]
        source: io::Error,
    },
    /// Snapshot artifact bytes or metadata did not match the manifest.
    #[error(
        "snapshot artifact {kind:?} at {} mismatch: expected sha256 {expected_sha256}, got {actual_sha256}",
        path.display()
    )]
    ArtifactMismatch {
        /// Artifact kind that mismatched.
        kind: ArtifactKind,
        /// Artifact path.
        path: PathBuf,
        /// Manifest sha256.
        expected_sha256: String,
        /// Recomputed sha256.
        actual_sha256: String,
    },
    /// Snapshot artifact set digest did not match the manifest.
    #[error(
        "snapshot artifact set mismatch: expected sha256 {expected_sha256}, got {actual_sha256}"
    )]
    ArtifactSetMismatch {
        /// Manifest artifact-set sha256.
        expected_sha256: String,
        /// Recomputed artifact-set sha256.
        actual_sha256: String,
    },
    /// The manifest omitted a required artifact kind.
    #[error("snapshot manifest missing required artifact {kind:?}")]
    ManifestMissingArtifact {
        /// Missing artifact kind.
        kind: ArtifactKind,
    },
    /// The manifest artifact set has the wrong cardinality or shape.
    #[error("invalid snapshot manifest artifact set: {detail}")]
    ManifestArtifactSetInvalid {
        /// Rejection detail.
        detail: String,
    },
    /// The snapshot was captured against a different Firecracker version.
    #[error("snapshot Firecracker version mismatch: expected {expected}, recorded {recorded}")]
    FirecrackerVersionMismatch {
        /// Restore environment expected version.
        expected: String,
        /// Manifest-recorded capture version.
        recorded: String,
    },
    /// Snapshot paths were not a usable two-file pair.
    #[error("invalid snapshot paths: {detail}")]
    InvalidSnapshotPaths {
        /// Rejection detail.
        detail: String,
    },
    /// System clock could not produce a Unix timestamp for manifest creation.
    #[error("snapshot manifest clock error: {detail}")]
    Clock {
        /// Clock error detail.
        detail: String,
    },
    /// The vsock UDS file could not be removed before restore. `ENOENT` is
    /// never returned here — only errors other than `NotFound`.
    #[error("vsock UDS unlink failed at {}: {source}", path.display())]
    VsockUdsUnlink {
        /// Path that could not be removed.
        path: PathBuf,
        /// Underlying I/O error (never `NotFound`).
        #[source]
        source: io::Error,
    },
    /// Caller-supplied identifier would escape or hide inside the persistence
    /// path layout.
    #[error("invalid snapshot persistence id for {field}: {value:?}")]
    InvalidId {
        /// Field whose value was rejected.
        field: &'static str,
        /// Rejected identifier.
        value: String,
    },
}

/// Errors surfaced by snapshot schema helpers.
#[derive(Debug, thiserror::Error)]
pub enum SchemaError {
    /// `schema_version` in the file did not match [`SCHEMA_VERSION`].
    #[error("unsupported snapshot schema version: got {0}, expected {SCHEMA_VERSION}")]
    UnsupportedSchemaVersion(u32),
    /// Underlying I/O failure; carries the path so tests can pin which file
    /// failed even while the schema helpers stay private.
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

// ---------------------------------------------------------------------------
// Schema helpers
// ---------------------------------------------------------------------------

/// Partial-deserialize struct used to extract `schema_version` before
/// committing to a full parse. Without this, a v0.2 file stamped
/// `schema_version: 2` plus a new field surfaces as a
/// `Json("unknown field …")` (because the structs carry
/// `#[serde(deny_unknown_fields)]`) instead of `UnsupportedSchemaVersion(2)`.
#[derive(Deserialize)]
struct SchemaVersionProbe {
    schema_version: u32,
}

fn wrap_io_err(path: &Path) -> impl Fn(io::Error) -> SchemaError + '_ {
    |source| SchemaError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// Write `value` to `path` as pretty JSON + trailing newline + mode 0644.
fn write_pretty_json_0644<T: Serialize>(value: &T, path: &Path) -> Result<(), SchemaError> {
    let mut json = serde_json::to_string_pretty(value).map_err(SchemaError::Json)?;
    json.push('\n');
    std::fs::write(path, json.as_bytes()).map_err(wrap_io_err(path))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o644))
            .map_err(wrap_io_err(path))?;
    }
    Ok(())
}

/// Probe `schema_version` before full parse so a v0.2 file reports
/// `UnsupportedSchemaVersion(2)` instead of leaking the unrelated
/// `Json("unknown field …")` from `deny_unknown_fields`.
fn parse_with_schema_probe<T: DeserializeOwned>(raw: &[u8]) -> Result<T, SchemaError> {
    let probe: SchemaVersionProbe = serde_json::from_slice(raw).map_err(SchemaError::Json)?;
    if probe.schema_version != SCHEMA_VERSION {
        return Err(SchemaError::UnsupportedSchemaVersion(probe.schema_version));
    }
    serde_json::from_slice(raw).map_err(SchemaError::Json)
}

fn read_with_schema_probe<T: DeserializeOwned>(path: &Path) -> Result<T, SchemaError> {
    let raw = std::fs::read(path).map_err(wrap_io_err(path))?;
    parse_with_schema_probe(&raw)
}

impl SnapshotManifest {
    /// Write to `path` as pretty JSON + trailing newline + mode 0644 (Unix).
    pub fn write(&self, path: &Path) -> Result<(), SchemaError> {
        write_pretty_json_0644(self, path)
    }

    /// Parse and structurally validate a manifest from raw bytes. Probes
    /// `schema_version` before full parse so future-version payloads report
    /// `UnsupportedSchemaVersion` instead of leaking `Json("unknown field …")`
    /// from `deny_unknown_fields`. sha256s are NOT checked here.
    pub fn from_bytes(raw: &[u8]) -> Result<SnapshotManifest, SchemaError> {
        parse_with_schema_probe(raw)
    }

    /// Read and structurally validate a manifest at `path`. Probes
    /// `schema_version` before full parse; sha256s are NOT checked here.
    pub fn read(path: &Path) -> Result<SnapshotManifest, SchemaError> {
        read_with_schema_probe(path)
    }
}

#[cfg(test)]
impl RestoreMetadata {
    /// Write to `path` as pretty JSON + trailing newline + mode 0644 (Unix).
    fn write(&self, path: &Path) -> Result<(), SchemaError> {
        write_pretty_json_0644(self, path)
    }

    /// Read and structurally validate restore metadata at `path`.
    fn read(path: &Path) -> Result<RestoreMetadata, SchemaError> {
        read_with_schema_probe(path)
    }
}

// ---------------------------------------------------------------------------
// Path helpers
// ---------------------------------------------------------------------------

/// Compute the canonical persistence path for a snapshot.
///
/// Format: `<store_root>/<workspace_id>/<run_id>/<created_at_unix_ms>-<artifact_set_sha256>/`.
///
/// Pure path construction; no I/O. Collision detection is a capture-time
/// concern for the future orchestrator persistence path.
///
/// The `<store_root>` must be a host-local filesystem path. m80 ships no
/// remote-store support; adding S3/GCS/generic stores is a v0.2+ epic.
///
/// `workspace_id` and `run_id` are single path components. Empty values,
/// hidden-dot values, path separators, NUL bytes, and `..` traversal markers
/// are rejected before any [`Path::join`] call.
///
/// # Errors
///
/// Returns [`SnapshotError::InvalidId`] when either caller-supplied id is not
/// one safe path component.
pub fn persistence_path(
    store_root: &Path,
    workspace_id: &str,
    run_id: &str,
    created_at_unix_ms: u64,
    artifact_set_sha256: &str,
) -> Result<PathBuf, SnapshotError> {
    validate_persistence_id("workspace_id", workspace_id)?;
    validate_persistence_id("run_id", run_id)?;
    Ok(store_root
        .join(workspace_id)
        .join(run_id)
        .join(format!("{created_at_unix_ms}-{artifact_set_sha256}")))
}

fn validate_persistence_id(field: &'static str, value: &str) -> Result<(), SnapshotError> {
    if value.is_empty()
        || value.starts_with('.')
        || value.contains('/')
        || value.contains('\\')
        || value.contains('\0')
        || value.contains("..")
    {
        return Err(SnapshotError::InvalidId {
            field,
            value: value.to_owned(),
        });
    }
    Ok(())
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
pub(crate) fn artifact_set_sha256(artifacts: &[Artifact]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    for artifact in artifacts {
        let bytes = serde_json::to_vec(artifact).expect("Artifact serialization is infallible");
        hasher.update(&bytes);
    }
    hasher.finalize().into()
}

#[cfg(test)]
mod tests;
