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

use m80_snapshot::{Artifact, ArtifactKind, RestoreMetadata, SnapshotManifest, SCHEMA_VERSION};

/// Return the five required artifacts pointing at `dir`-relative paths.
/// All sha256 and size values are fixed constants — no real files needed.
pub fn five_artifacts(dir: &Path) -> Vec<Artifact> {
    vec![
        Artifact {
            kind: ArtifactKind::BootIdentity,
            path: dir.join("boot-identity.bin"),
            sha256: "a".repeat(64),
            size: 512,
        },
        Artifact {
            kind: ArtifactKind::Memory,
            path: dir.join("snapshot-memory.bin"),
            sha256: "b".repeat(64),
            size: 134217728,
        },
        Artifact {
            kind: ArtifactKind::RuntimeRootfs,
            path: dir.join("runtime-rootfs.ext4"),
            sha256: "c".repeat(64),
            size: 536870912,
        },
        Artifact {
            kind: ArtifactKind::VmState,
            path: dir.join("snapshot-vmstate.bin"),
            sha256: "d".repeat(64),
            size: 4096,
        },
        Artifact {
            kind: ArtifactKind::WorkspaceScratch,
            path: dir.join("workspace-scratch.ext4"),
            sha256: "e".repeat(64),
            size: 1073741824,
        },
    ]
}

/// Build a minimal valid `SnapshotManifest` whose artifact paths sit under
/// `dir`. Tests that only care about round-trip behavior use this.
pub fn sample_manifest(dir: &Path) -> SnapshotManifest {
    let arts = five_artifacts(dir);
    let sha = hex::encode(m80_snapshot::artifact_set_sha256(&arts));
    SnapshotManifest {
        artifact_set_sha256: sha,
        artifacts: arts,
        created_at_unix_ms: 1_700_000_000_000,
        diagnostics_bundle: None,
        expected_firecracker_version: "v1.15.1".into(),
        metrics_snapshot: None,
        schema_version: SCHEMA_VERSION,
        source_run_id: "run-abc".into(),
        source_vm_id: "vm-xyz".into(),
        source_workspace_id: "ws-123".into(),
    }
}

/// Build a minimal valid `RestoreMetadata`.
pub fn sample_restore_metadata(dir: &Path) -> RestoreMetadata {
    RestoreMetadata {
        expected_firecracker_version: "v1.15.1".into(),
        schema_version: SCHEMA_VERSION,
        snapshot_path: dir.join("snap"),
        source_run_id: "run-abc".into(),
        source_vm_id: "vm-xyz".into(),
        source_workspace_id: "ws-123".into(),
    }
}
