//! Bead m80-sz1.3.2 — schema_version + firecracker version stamped in manifest
//!
//! Doc anchor: `docs/behaviors/image-build/manifest.md#schema-version`

mod common;

use m80_image_manifest::{Manifest, ManifestError, SCHEMA_VERSION};

/// Bead m80-sz1.3.2: the manifest carries `schema_version: 1` and
/// `expected_firecracker_version`.
#[test]
fn stamps_schema_and_firecracker_version() {
    let dir = tempfile::tempdir().unwrap();
    let m = common::make_artifacts(dir.path());
    let path = dir.path().join("rootfs.ext4.manifest.json");
    m.write(&path).unwrap();

    let m2 = Manifest::read(&path).unwrap();
    assert_eq!(m2.schema_version, SCHEMA_VERSION);
    assert_eq!(m2.expected_firecracker_version, "v1.15.1");
}

/// Reading a manifest with an unsupported schema_version returns the right error.
#[test]
fn wrong_schema_version_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let m = common::make_artifacts(dir.path());
    let path = dir.path().join("bad_schema.json");

    let mut v = serde_json::to_value(&m).unwrap();
    v["schema_version"] = serde_json::json!(99u32);
    let raw = format!("{}\n", serde_json::to_string_pretty(&v).unwrap());
    std::fs::write(&path, raw.as_bytes()).unwrap();

    let err = Manifest::read(&path).unwrap_err();
    assert!(
        matches!(err, ManifestError::UnsupportedSchemaVersion(99)),
        "expected UnsupportedSchemaVersion(99), got {err:?}"
    );
}
