//! Bead m80-6a0q.4 — kind/field invariants on Manifest
//!
//! Doc anchor: `docs/behaviors/image-build/minimal-image-design.md`
//!
//! `Ubuntu` requires source-rootfs Options populated; `Minimal` requires
//! those `None`. Asymmetric population on either side is a hard error.

mod common;

use m80_image_manifest::{ImageKind, Manifest, ManifestError};

#[test]
fn ubuntu_with_all_options_populated_passes_verify() {
    let dir = tempfile::tempdir().unwrap();
    let m = common::make_artifacts(dir.path());
    m.verify(dir.path()).unwrap();
}

#[test]
fn minimal_with_all_options_none_passes_verify() {
    let dir = tempfile::tempdir().unwrap();
    let m = common::make_minimal_artifacts(dir.path());
    m.verify(dir.path()).unwrap();
}

#[test]
fn ubuntu_with_source_rootfs_field_none_is_inconsistent() {
    let dir = tempfile::tempdir().unwrap();
    let mut m = common::make_artifacts(dir.path());
    m.source_rootfs_image = None;
    let err = m.verify(dir.path()).unwrap_err();
    assert!(
        matches!(
            &err,
            ManifestError::InconsistentKind {
                kind: ImageKind::Ubuntu,
                field,
                expected: "Some(_)",
            } if field == "source_rootfs_image"
        ),
        "expected InconsistentKind(Ubuntu, source_rootfs_image, Some(_)), got {err:?}"
    );
}

#[test]
fn minimal_with_source_rootfs_set_is_inconsistent() {
    let dir = tempfile::tempdir().unwrap();
    let mut m = common::make_minimal_artifacts(dir.path());
    m.source_rootfs_image = Some(dir.path().join("source.ext4"));
    let err = m.verify(dir.path()).unwrap_err();
    assert!(
        matches!(
            &err,
            ManifestError::InconsistentKind {
                kind: ImageKind::Minimal,
                field,
                expected: "None",
            } if field == "source_rootfs_image"
        ),
        "expected InconsistentKind(Minimal, source_rootfs_image, None), got {err:?}"
    );
}

#[test]
fn write_then_read_preserves_kind() {
    let dir = tempfile::tempdir().unwrap();
    let m = common::make_minimal_artifacts(dir.path());
    let path = dir.path().join("minimal.manifest.json");
    m.write(&path).unwrap();
    let m2 = Manifest::read(&path).unwrap();
    assert_eq!(m2.image_kind, ImageKind::Minimal);
    assert!(m2.source_rootfs_image.is_none());
    assert!(m2.source_rootfs_sha256.is_none());
}
