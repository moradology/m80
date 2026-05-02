//! Bead m80-sz1.3.1 — manifest written beside rootfs
//!
//! Doc anchor: `docs/behaviors/image-build/manifest.md#emit`

mod common;

use std::path::PathBuf;

use m80_image_manifest::Manifest;

/// Bead m80-sz1.3.1: `Manifest::write` places the manifest beside the rootfs
/// as `<rootfs>.manifest.json` with mode 0644.
#[test]
fn emits_manifest_beside_rootfs() {
    let dir = tempfile::tempdir().unwrap();
    let m = common::make_artifacts(dir.path());

    let manifest_path = PathBuf::from(format!(
        "{}.manifest.json",
        m.output_rootfs_image.display()
    ));
    m.write(&manifest_path).unwrap();

    assert!(manifest_path.exists(), "manifest file must exist beside rootfs");

    let m2 = Manifest::read(&manifest_path).unwrap();
    assert_eq!(m, m2, "round-tripped manifest must equal the original");

    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let mode = std::fs::metadata(&manifest_path).unwrap().mode() & 0o777;
        assert_eq!(mode, 0o644, "manifest mode must be 0644, got {mode:o}");
    }
}
