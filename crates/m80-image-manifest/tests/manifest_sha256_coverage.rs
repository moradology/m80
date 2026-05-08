//! Bead m80-sz1.3.3 — sha256 coverage for all current artifact slots
//!
//! Doc anchor: `docs/behaviors/image-build/manifest.md#sha256-coverage`

mod common;

use m80_image_manifest::{Manifest, ManifestError};

/// Bead m80-sz1.3.3: all current artifact hash slots are populated and `verify()`
/// validates them. Mutating any single hash field fires `Sha256Mismatch` on
/// the corresponding path field.
#[test]
fn sha256_covers_all_inputs() {
    let dir = tempfile::tempdir().unwrap();
    let m = common::make_artifacts(dir.path());

    m.verify(dir.path()).unwrap();

    for (mutate, expected_field) in [
        (
            (|m: &mut Manifest| m.kernel_image_sha256 = "aa".repeat(32)) as fn(&mut Manifest),
            "kernel_image",
        ),
        (
            (|m| m.source_rootfs_sha256 = Some("bb".repeat(32))),
            "source_rootfs_image",
        ),
        (
            (|m| m.output_rootfs_sha256 = "cc".repeat(32)),
            "output_rootfs_image",
        ),
        (
            (|m| m.daemon_binary_sha256 = "dd".repeat(32)),
            "daemon_binary_path",
        ),
    ] {
        let mut bad = m.clone();
        mutate(&mut bad);
        let err = bad.verify(dir.path()).unwrap_err();
        assert!(
            matches!(&err, ManifestError::Sha256Mismatch { field, .. } if field == expected_field),
            "expected Sha256Mismatch on {expected_field}, got {err:?}"
        );
    }
}
