//! Bead m80-sz1.4.1 — schema validation at boot rejects invalid manifests
//!
//! Doc anchor: `docs/behaviors/image-build/manifest-verify.md#schema-check`
//!
//! Five distinct rejection scenarios, each its own `#[test]` so a failure in
//! one doesn't mask the others.

mod common;

use std::path::Path;

use m80_image_manifest::{Manifest, ManifestError};

fn write_with_mutation(
    dir: &Path,
    name: &str,
    mutate: impl FnOnce(&mut serde_json::Value),
) -> std::path::PathBuf {
    let m = common::make_artifacts(dir);
    let mut v = serde_json::to_value(&m).unwrap();
    mutate(&mut v);
    let raw = format!("{}\n", serde_json::to_string_pretty(&v).unwrap());
    let path = dir.join(name);
    std::fs::write(&path, raw.as_bytes()).unwrap();
    path
}

/// Bead m80-sz1.4.1: future schema_version is rejected as
/// `UnsupportedSchemaVersion`, not as a structural parse error.
#[test]
fn future_schema_version_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_with_mutation(dir.path(), "future_schema.json", |v| {
        v["schema_version"] = serde_json::json!(42u32);
    });
    let err = Manifest::read(&path).unwrap_err();
    assert!(
        matches!(err, ManifestError::UnsupportedSchemaVersion(42)),
        "expected UnsupportedSchemaVersion(42), got {err:?}"
    );
}

/// schema_version = 0 is also rejected as `UnsupportedSchemaVersion`.
#[test]
fn zero_schema_version_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_with_mutation(dir.path(), "zero_schema.json", |v| {
        v["schema_version"] = serde_json::json!(0u32);
    });
    let err = Manifest::read(&path).unwrap_err();
    assert!(
        matches!(err, ManifestError::UnsupportedSchemaVersion(0)),
        "expected UnsupportedSchemaVersion(0), got {err:?}"
    );
}

/// Missing required field surfaces as a `Json` parse error.
#[test]
fn missing_required_field_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_with_mutation(dir.path(), "missing_field.json", |v| {
        v.as_object_mut().unwrap().remove("boot_target");
    });
    let err = Manifest::read(&path).unwrap_err();
    assert!(
        matches!(err, ManifestError::Json(_)),
        "expected Json error for missing field, got {err:?}"
    );
}

/// Unknown field present is rejected by `deny_unknown_fields`.
#[test]
fn unknown_field_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_with_mutation(dir.path(), "unknown_field.json", |v| {
        v["UNKNOWN_KEY"] = serde_json::json!("surprise");
    });
    let err = Manifest::read(&path).unwrap_err();
    assert!(
        matches!(err, ManifestError::Json(_)),
        "expected Json error for unknown field, got {err:?}"
    );
}

/// Regression: a future-version manifest with extra fields surfaces as
/// `UnsupportedSchemaVersion`, not `Json("unknown field …")`. The probe
/// fires before `deny_unknown_fields`.
#[test]
fn schema_version_check_fires_before_unknown_field_check() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_with_mutation(dir.path(), "future_with_unknown.json", |v| {
        v["schema_version"] = serde_json::json!(2u32);
        v.as_object_mut()
            .unwrap()
            .insert("future_field".into(), serde_json::json!("v0.2 stuff"));
    });
    let err = Manifest::read(&path).unwrap_err();
    assert!(
        matches!(err, ManifestError::UnsupportedSchemaVersion(2)),
        "expected UnsupportedSchemaVersion(2), got {err:?}"
    );
}

/// A valid manifest reads without error (sanity check that the helper isn't
/// systematically broken).
#[test]
fn valid_manifest_reads_clean() {
    let dir = tempfile::tempdir().unwrap();
    let m = common::make_artifacts(dir.path());
    let path = dir.path().join("valid.json");
    m.write(&path).unwrap();
    Manifest::read(&path).unwrap();
}
