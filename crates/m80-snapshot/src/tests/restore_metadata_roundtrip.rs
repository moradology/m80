//! Round-trip tests for `RestoreMetadata`.
//! Bead: m80-0tf.1.3

use super::common;
use crate::{RestoreMetadata, SchemaError, RESTORE_METADATA_FILE};

/// `write` then `read` produces a struct equal to the original.
#[test]
fn round_trip_equals_original() {
    let dir = tempfile::tempdir().unwrap();
    let m = common::sample_restore_metadata(dir.path());
    let path = dir.path().join(RESTORE_METADATA_FILE);
    common::assert_round_trips(m, &path, |v, p| v.write(p), RestoreMetadata::read);
}

/// Writing twice produces byte-identical files.
#[test]
fn round_trip_is_byte_stable() {
    let dir = tempfile::tempdir().unwrap();
    let m = common::sample_restore_metadata(dir.path());

    let p1 = dir.path().join("m1.json");
    let p2 = dir.path().join("m2.json");
    m.write(&p1).unwrap();
    let m2 = RestoreMetadata::read(&p1).unwrap();
    m2.write(&p2).unwrap();

    let raw1 = std::fs::read(&p1).unwrap();
    let raw2 = std::fs::read(&p2).unwrap();
    assert_eq!(raw1, raw2, "re-serialized bytes must be identical");
}

// trailing-newline + file-mode tests are pinned by manifest_roundtrip.rs;
// both types share `write_pretty_json_0644` so verifying once suffices.

/// Wrong schema version returns `UnsupportedSchemaVersion`.
#[test]
fn wrong_schema_version_returns_unsupported() {
    let dir = tempfile::tempdir().unwrap();
    let m = common::sample_restore_metadata(dir.path());
    let mut v = serde_json::to_value(&m).unwrap();
    v["schema_version"] = serde_json::json!(42u32);
    let raw = format!("{}\n", serde_json::to_string_pretty(&v).unwrap());
    let path = dir.path().join("bad.json");
    std::fs::write(&path, raw.as_bytes()).unwrap();
    let err = RestoreMetadata::read(&path).unwrap_err();
    assert!(
        matches!(err, SchemaError::UnsupportedSchemaVersion(42)),
        "expected UnsupportedSchemaVersion(42), got {err:?}"
    );
}

/// `deny_unknown_fields` rejects extra keys.
#[test]
fn deny_unknown_fields_rejects_extra_key() {
    let dir = tempfile::tempdir().unwrap();
    let m = common::sample_restore_metadata(dir.path());
    let path = dir.path().join("extra.json");

    let mut v = serde_json::to_value(&m).unwrap();
    v.as_object_mut()
        .unwrap()
        .insert("EXTRA_FIELD".into(), serde_json::json!("forbidden"));
    let raw = format!("{}\n", serde_json::to_string_pretty(&v).unwrap());
    std::fs::write(&path, raw.as_bytes()).unwrap();

    let err = RestoreMetadata::read(&path).unwrap_err();
    let SchemaError::Json(ref je) = err else {
        panic!("expected SchemaError::Json, got {err:?}");
    };
    assert!(
        je.to_string().contains("unknown field"),
        "expected 'unknown field' in serde error, got: {je}"
    );
}

/// `read` fails with `Io { path, .. }` for a missing file.
#[test]
fn read_missing_file_surfaces_io_error_with_path() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("missing.json");
    let err = RestoreMetadata::read(&path).unwrap_err();
    match err {
        SchemaError::Io { path: p, source } => {
            assert_eq!(p, path);
            assert_eq!(source.kind(), std::io::ErrorKind::NotFound);
        }
        other => panic!("expected Io error with path, got {other:?}"),
    }
}

/// JSON keys are in alphabetical order.
#[test]
fn json_keys_are_alphabetical() {
    let dir = tempfile::tempdir().unwrap();
    let m = common::sample_restore_metadata(dir.path());
    let path = dir.path().join("key_order.json");
    m.write(&path).unwrap();
    let raw = std::fs::read(&path).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&raw).unwrap();
    let keys: Vec<String> = v.as_object().unwrap().keys().cloned().collect();
    let mut sorted = keys.clone();
    sorted.sort_unstable();
    assert_eq!(keys, sorted, "JSON keys must be in alphabetical order");
}
