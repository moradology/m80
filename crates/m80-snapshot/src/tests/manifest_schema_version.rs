//! Schema version gate tests for `SnapshotManifest`.
//! Bead: m80-0tf.1.1

use super::common;
use crate::{SchemaError, SnapshotManifest, SCHEMA_VERSION};

fn bytes_with_version(dir: &std::path::Path, version: u32) -> Vec<u8> {
    let m = common::sample_manifest(dir);
    let mut v = serde_json::to_value(&m).unwrap();
    v["schema_version"] = serde_json::json!(version);
    serde_json::to_vec_pretty(&v).unwrap()
}

/// Wrong schema version returns `UnsupportedSchemaVersion(N)`.
#[test]
fn wrong_schema_version_returns_unsupported() {
    let dir = tempfile::tempdir().unwrap();
    let raw = bytes_with_version(dir.path(), 99);
    let err = SnapshotManifest::from_bytes(&raw).unwrap_err();
    assert!(
        matches!(err, SchemaError::UnsupportedSchemaVersion(99)),
        "expected UnsupportedSchemaVersion(99), got {err:?}"
    );
}

#[test]
fn malformed_json_returns_json_error_from_from_bytes() {
    let err = SnapshotManifest::from_bytes(br#"{"schema_version":1,"artifacts":["#).unwrap_err();
    let SchemaError::Json(json) = err else {
        panic!("expected SchemaError::Json for malformed bytes, got {err:?}");
    };
    assert!(
        json.is_eof() || json.is_syntax(),
        "malformed JSON should surface as serde_json syntax/eof, got {json}"
    );
}

/// Version 0 (predates v0.1 schema) is also rejected.
#[test]
fn version_zero_returns_unsupported() {
    let dir = tempfile::tempdir().unwrap();
    let raw = bytes_with_version(dir.path(), 0);
    let err = SnapshotManifest::from_bytes(&raw).unwrap_err();
    assert!(
        matches!(err, SchemaError::UnsupportedSchemaVersion(0)),
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
    let raw = serde_json::to_vec_pretty(&v).unwrap();

    let err = SnapshotManifest::from_bytes(&raw).unwrap_err();
    assert!(
        matches!(err, SchemaError::UnsupportedSchemaVersion(2)),
        "expected UnsupportedSchemaVersion(2), got {err:?}"
    );
}

/// Correct schema version 1 round-trips through bytes successfully.
#[test]
fn correct_schema_version_reads_ok() {
    let dir = tempfile::tempdir().unwrap();
    let raw = bytes_with_version(dir.path(), SCHEMA_VERSION);
    SnapshotManifest::from_bytes(&raw).expect("schema_version=1 must parse successfully");
}
