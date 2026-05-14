//! Shared test helpers for `m80-snapshot` integration tests.
//!
//! Each `tests/*.rs` file is its own integration crate, so we share via
//! `mod common;` rather than via `pub use`.
//!
//! Helpers that are not consumed by every test binary look unused to each
//! binary's view of dead-code. Suppress the lint here rather than adding
//! `#[allow]` at every call site.
#![allow(dead_code)]

use std::path::Path;

use crate::{
    artifact_set_sha256, Artifact, ArtifactKind, RestoreMetadata, SchemaError, SnapshotManifest,
    SCHEMA_VERSION,
};

/// Return the required snapshot artifacts pointing at `dir`-relative paths.
/// All sha256 and size values are fixed constants — no real files needed.
pub(crate) fn snapshot_artifacts(dir: &Path) -> Vec<Artifact> {
    vec![
        Artifact {
            kind: ArtifactKind::Memory,
            path: dir.join("snapshot-memory.bin"),
            sha256: "b".repeat(64),
            size: 134217728,
        },
        Artifact {
            kind: ArtifactKind::VmState,
            path: dir.join("snapshot-vmstate.bin"),
            sha256: "d".repeat(64),
            size: 4096,
        },
    ]
}

/// Build a minimal valid `SnapshotManifest` whose artifact paths sit under
/// `dir`. Tests that only care about round-trip behavior use this.
pub(crate) fn sample_manifest(dir: &Path) -> SnapshotManifest {
    let arts = snapshot_artifacts(dir);
    let sha = hex::encode(artifact_set_sha256(&arts));
    SnapshotManifest {
        artifact_set_sha256: sha,
        artifacts: arts,
        created_at_unix_ms: 1_700_000_000_000,
        expected_firecracker_version: "v1.15.1".into(),
        schema_version: SCHEMA_VERSION,
    }
}

/// Build a minimal valid `RestoreMetadata`.
pub(crate) fn sample_restore_metadata(dir: &Path) -> RestoreMetadata {
    RestoreMetadata {
        expected_firecracker_version: "v1.15.1".into(),
        schema_version: SCHEMA_VERSION,
        snapshot_path: dir.join("snap"),
        source_run_id: "run-abc".into(),
        source_vm_id: "vm-xyz".into(),
        source_workspace_id: "ws-123".into(),
    }
}

/// Assert that a value survives a write→read round-trip unchanged.
///
/// `write` and `read` are the type-specific persistence closures.
pub(crate) fn assert_round_trips<T>(
    value: T,
    path: &std::path::Path,
    write: impl Fn(&T, &std::path::Path) -> Result<(), SchemaError>,
    read: impl Fn(&std::path::Path) -> Result<T, SchemaError>,
) where
    T: PartialEq + std::fmt::Debug,
{
    write(&value, path).expect("write must succeed");
    let restored = read(path).expect("read must succeed");
    assert_eq!(
        value, restored,
        "round-tripped value must equal the original"
    );
}
