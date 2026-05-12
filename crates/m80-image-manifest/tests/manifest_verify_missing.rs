//! `Manifest::verify` surfaces `Io { path, source: NotFound }` for each of
//! the current artifact slots when its file is missing. The unit test in
//! `src/lib.rs` only covers `kernel_image`; this file pins the others so we
//! don't ship a half-tested error path.

mod common;

use std::io;
use std::path::PathBuf;

use m80_image_manifest::{Manifest, ManifestError};

fn assert_missing_field<F>(field: &str, mutate: F)
where
    F: FnOnce(&mut Manifest, &PathBuf),
{
    let dir = tempfile::tempdir().unwrap();
    let mut m = common::make_artifacts(dir.path());
    let missing = dir.path().join(format!("missing-{field}"));
    mutate(&mut m, &missing);
    let err = m.verify(dir.path()).unwrap_err();
    match err {
        ManifestError::Io { path, source } => {
            assert_eq!(path, missing, "Io must carry the failed path for {field}");
            assert_eq!(source.kind(), io::ErrorKind::NotFound);
        }
        other => panic!("{field}: expected Io NotFound, got {other:?}"),
    }
}

#[test]
fn missing_source_rootfs_surfaces_io() {
    assert_missing_field("source_rootfs_image", |m, p| {
        m.source_rootfs_image = Some(p.clone());
    });
}

#[test]
fn missing_output_rootfs_surfaces_io() {
    assert_missing_field("output_rootfs_image", |m, p| {
        m.output_rootfs_image = p.clone();
    });
}

#[test]
fn missing_daemon_binary_surfaces_io() {
    assert_missing_field("daemon_binary_path", |m, p| {
        m.daemon_binary_path = p.clone();
    });
}
