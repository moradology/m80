//! Schema version gate tests for `SnapshotManifest`.
//! Bead: m80-0tf.1.1

mod common;

use m80_snapshot::{SnapshotManifest, SnapshotError};

fn write_with_version(dir: &std::path::Path, version: u32) -> std::path::PathBuf {
    let m = common::sample_manifest(dir);
    let mut v = serde_json::to_value(&m).unwrap();
    v["schema_version"] = serde_json::json!(version);
    let raw = format!("{}\n", serde_json::to_string_pretty(&v).unwrap());
    let path = dir.join("manifest.json");
    std::fs::write(&path, raw.as_bytes()).unwrap();
    path
}

/// Wrong schema version returns `UnsupportedSchemaVersion(N)`.
#[test]
fn wrong_schema_version_returns_unsupported() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_with_version(dir.path(), 99);
    let err = SnapshotManifest::read(&path).unwrap_err();
    assert!(
        matches!(err, SnapshotError::UnsupportedSchemaVersion(99)),
        "expected UnsupportedSchemaVersion(99), got {err:?}"
    );
}

/// Version 0 (predates v0.1 schema) is also rejected.
#[test]
fn version_zero_returns_unsupported() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_with_version(dir.path(), 0);
    let err = SnapshotManifest::read(&path).unwrap_err();
    assert!(
        matches!(err, SnapshotError::UnsupportedSchemaVersion(0)),
        "expected UnsupportedSchemaVersion(0), got {err:?}"
    );
}

/// A future manifest with extra fields surfaces as `UnsupportedSchemaVersion`,
/// not `Json("unknown field …")`. The probe fires before `deny_unknown_fields`.
#[test]
fn schema_version_check_fires_before_unknown_field_check() {
    let dir = tempfile::tempdir().unwrap();
    let m = common::sample_manifest(dir.path());
    let mut v = serde_json::to_value(&m).unwrap();
    v["schema_version"] = serde_json::json!(2u32);
    v.as_object_mut()
        .unwrap()
        .insert("future_field".into(), serde_json::json!("v0.2 stuff"));
    let raw = format!("{}\n", serde_json::to_string_pretty(&v).unwrap());
    let path = dir.path().join("future.json");
    std::fs::write(&path, raw.as_bytes()).unwrap();

    let err = SnapshotManifest::read(&path).unwrap_err();
    assert!(
        matches!(err, SnapshotError::UnsupportedSchemaVersion(2)),
        "expected UnsupportedSchemaVersion(2), got {err:?}"
    );
}

/// Correct schema version 1 reads successfully.
#[test]
fn correct_schema_version_reads_ok() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_with_version(dir.path(), 1);
    SnapshotManifest::read(&path).expect("schema_version=1 must parse successfully");
}
