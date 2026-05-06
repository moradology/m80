//! Snapshot manifest schemas, persistence-path layout, and capture/restore
//! primitives for Firecracker microVM snapshots.
//!
//! See `README.md` for the full contract.
//! Behavior captures: bead epic `m80-0tf`; capture/restore: `m80-rrp.3.13`.

#![deny(missing_docs)]

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
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
pub const RESTORE_METADATA_FILE: &str = "restore-metadata.json";

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

/// Which type of snapshot to create.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotKind {
    /// Full snapshot — all guest memory pages are saved.
    Full,
    /// Diff snapshot — only pages dirtied since the previous snapshot.
    Diff,
}

/// Parameters for [`capture`].
pub struct CaptureRequest<'a> {
    /// Path to the Firecracker API socket.
    pub fc_socket: &'a Path,
    /// On-disk paths for the snapshot artifact pair.
    pub paths: SnapshotPaths,
    /// Full or Diff snapshot.
    pub kind: SnapshotKind,
}

/// Parameters for [`restore`].
pub struct RestoreRequest {
    /// Path to the Firecracker API socket for the **new** (restore-target) process.
    pub fc_socket: PathBuf,
    /// On-disk paths for the snapshot artifact pair.
    pub paths: SnapshotPaths,
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
///
/// The VM is left in the `Paused` state after a successful call. The caller
/// decides whether to resume or kill the Firecracker process.
///
/// # Errors
///
/// Returns [`SnapshotError::Client`] if either REST call fails.
pub fn capture(req: CaptureRequest<'_>) -> Result<(), SnapshotError> {
    let client = FirecrackerClient::new(req.fc_socket).map_err(SnapshotError::Client)?;

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

    Ok(())
}

/// Load a snapshot into a new Firecracker process, optionally resuming it.
///
/// Steps:
/// 1. Remove `req.vsock_uds` if present (`ENOENT` is silently ignored;
///    any other error is returned as [`SnapshotError::VsockUdsUnlink`]).
/// 2. PUT `/snapshot/load` with File-backed memory.
/// 3. If `req.resume`: PATCH `/vm` to `Resumed`.
///
/// # Errors
///
/// Returns [`SnapshotError::VsockUdsUnlink`] if the UDS file cannot be
/// removed for a reason other than `NotFound`.
/// Returns [`SnapshotError::Client`] if a REST call fails.
pub fn restore(req: RestoreRequest) -> Result<(), SnapshotError> {
    // Step 1: Remove stale vsock UDS so Firecracker can rebind it.
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

    // Step 2: Load the snapshot.
    let client = FirecrackerClient::new(&req.fc_socket).map_err(SnapshotError::Client)?;

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

    // Step 3: Optionally resume.
    if req.resume {
        client
            .patch_vm_state(VmState::Resumed)
            .map_err(SnapshotError::Client)?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors surfaced by snapshot operations.
#[derive(Debug, thiserror::Error)]
pub enum SnapshotError {
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
    /// A Firecracker REST call failed.
    #[error("firecracker client: {0}")]
    Client(#[from] m80_firecracker_client::ClientError),
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
        return Err(SnapshotError::UnsupportedSchemaVersion(
            probe.schema_version,
        ));
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

// ---------------------------------------------------------------------------
// Path helpers
// ---------------------------------------------------------------------------

/// Compute the canonical persistence path for a snapshot.
///
/// Format: `<store_root>/<workspace_id>/<run_id>/<created_at_unix_ms>-<artifact_set_sha256>/`.
///
/// Pure path construction; no I/O. Collision detection is a capture-time
/// concern (`SnapshotError::DestinationCollision`).
///
/// The `<store_root>` must be a host-local filesystem path. m80 ships no
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
