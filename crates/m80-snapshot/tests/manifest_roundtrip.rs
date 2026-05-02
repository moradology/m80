//! Round-trip and emit tests for `SnapshotManifest`.
//! Beads: m80-0tf.1.1, m80-0tf.1.2

mod common;

use m80_snapshot::SnapshotManifest;

/// `write` then `read` produces a struct equal to the original.
#[test]
fn round_trip_equals_original() {
    let dir = tempfile::tempdir().unwrap();
    let m = common::sample_manifest(dir.path());
    let path = dir.path().join("snapshot-manifest.json");
    m.write(&path).unwrap();
    let m2 = SnapshotManifest::read(&path).unwrap();
    assert_eq!(m, m2, "round-tripped manifest must equal the original");
}

/// Writing twice produces byte-identical files (stable serialization).
#[test]
fn round_trip_is_byte_stable() {
    let dir = tempfile::tempdir().unwrap();
    let m = common::sample_manifest(dir.path());

    let p1 = dir.path().join("m1.json");
    let p2 = dir.path().join("m2.json");
    m.write(&p1).unwrap();
    let m2 = SnapshotManifest::read(&p1).unwrap();
    m2.write(&p2).unwrap();

    let raw1 = std::fs::read(&p1).unwrap();
    let raw2 = std::fs::read(&p2).unwrap();
    assert_eq!(raw1, raw2, "re-serialized bytes must be identical");
}

/// Output ends with a trailing newline.
#[test]
fn trailing_newline_present() {
    let dir = tempfile::tempdir().unwrap();
    let m = common::sample_manifest(dir.path());
    let path = dir.path().join("m.json");
    m.write(&path).unwrap();
    let raw = std::fs::read(&path).unwrap();
    assert_eq!(raw.last(), Some(&b'\n'), "output must end with a newline");
}

/// File permissions are 0644 on Unix.
#[test]
#[cfg(unix)]
fn file_mode_is_0644() {
    use std::os::unix::fs::MetadataExt;
    let dir = tempfile::tempdir().unwrap();
    let m = common::sample_manifest(dir.path());
    let path = dir.path().join("mode.json");
    m.write(&path).unwrap();
    let mode = std::fs::metadata(&path).unwrap().mode() & 0o777;
    assert_eq!(mode, 0o644, "manifest file mode must be 0644, got {mode:o}");
}

/// Top-level JSON keys are in alphabetical order (struct field order).
#[test]
fn top_level_json_keys_are_alphabetical() {
    let dir = tempfile::tempdir().unwrap();
    let m = common::sample_manifest(dir.path());
    let path = dir.path().join("key_order.json");
    m.write(&path).unwrap();
    let raw = std::fs::read(&path).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&raw).unwrap();
    let top_keys: Vec<String> = v
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    let mut sorted = top_keys.clone();
    sorted.sort_unstable();
    assert_eq!(top_keys, sorted, "top-level JSON keys must be alphabetical");
}

/// Keys within each artifact object are in alphabetical order.
#[test]
fn artifact_json_keys_are_alphabetical() {
    let dir = tempfile::tempdir().unwrap();
    let m = common::sample_manifest(dir.path());
    let path = dir.path().join("key_order.json");
    m.write(&path).unwrap();
    let raw = std::fs::read(&path).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&raw).unwrap();
    for (i, art) in v["artifacts"].as_array().unwrap().iter().enumerate() {
        let keys: Vec<String> = art.as_object().unwrap().keys().cloned().collect();
        let mut sorted = keys.clone();
        sorted.sort_unstable();
        assert_eq!(keys, sorted, "artifact[{i}] JSON keys must be alphabetical");
    }
}

/// `deny_unknown_fields` rejects a manifest with an extra key.
#[test]
fn deny_unknown_fields_rejects_extra_key() {
    let dir = tempfile::tempdir().unwrap();
    let m = common::sample_manifest(dir.path());
    let path = dir.path().join("extra.json");

    let mut v = serde_json::to_value(&m).unwrap();
    v.as_object_mut()
        .unwrap()
        .insert("EXTRA_FIELD".into(), serde_json::json!("forbidden"));
    let raw = format!("{}\n", serde_json::to_string_pretty(&v).unwrap());
    std::fs::write(&path, raw.as_bytes()).unwrap();

    let err = SnapshotManifest::read(&path).unwrap_err();
    assert!(
        matches!(err, m80_snapshot::SnapshotError::Json(_)),
        "unknown field must surface as Json error, got {err:?}"
    );
}

/// `write` fails with `Io { path, .. }` when the parent directory is missing.
#[test]
fn write_missing_parent_surfaces_io_error_with_path() {
    let dir = tempfile::tempdir().unwrap();
    let m = common::sample_manifest(dir.path());
    let path = dir.path().join("nonexistent").join("snapshot-manifest.json");
    let err = m.write(&path).unwrap_err();
    match err {
        m80_snapshot::SnapshotError::Io { path: p, .. } => {
            assert_eq!(p, path, "Io variant must carry the attempted path");
        }
        other => panic!("expected Io error with path, got {other:?}"),
    }
}

/// `read` fails with `Io { path, .. }` when the file is missing.
#[test]
fn read_missing_file_surfaces_io_error_with_path() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("does-not-exist.json");
    let err = SnapshotManifest::read(&path).unwrap_err();
    match err {
        m80_snapshot::SnapshotError::Io { path: p, source } => {
            assert_eq!(p, path);
            assert_eq!(source.kind(), std::io::ErrorKind::NotFound);
        }
        other => panic!("expected Io error with path, got {other:?}"),
    }
}

/// The five required artifact kinds survive a round-trip.
#[test]
fn five_required_artifact_kinds_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let m = common::sample_manifest(dir.path());
    assert_eq!(m.artifacts.len(), 5, "sample must contain exactly 5 artifacts");
    let path = dir.path().join("snapshot-manifest.json");
    m.write(&path).unwrap();
    let m2 = SnapshotManifest::read(&path).unwrap();
    assert_eq!(m2.artifacts.len(), 5);
    for (a, b) in m.artifacts.iter().zip(m2.artifacts.iter()) {
        assert_eq!(a.kind, b.kind);
        assert_eq!(a.sha256, b.sha256);
        assert_eq!(a.size, b.size);
    }
}
