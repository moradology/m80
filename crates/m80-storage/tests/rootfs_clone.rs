//! Tests for `Rootfs::clone` and `Rootfs::new_at`.

use std::io::Write;

use m80_storage::Rootfs;
use sha2::{Digest, Sha256};

fn sha256_file(path: &std::path::Path) -> String {
    let bytes = std::fs::read(path).expect("read file");
    hex::encode(Sha256::digest(&bytes))
}

#[test]
fn clone_produces_byte_identical_copy() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().join("base.ext4");
    let dest = dir.path().join("clone.ext4");

    // Write a recognisable payload — not a real ext4 but sufficient for clone.
    let mut f = std::fs::File::create(&base).unwrap();
    f.write_all(b"fake-rootfs-content-for-clone-test").unwrap();
    drop(f);

    let rootfs = Rootfs::clone(&base, &dest).expect("clone must succeed");

    assert_eq!(sha256_file(&base), sha256_file(&dest), "dest must be byte-identical to base");
    assert_eq!(rootfs.path(), dest.as_path(), "path() must return dest");
}

#[test]
fn clone_returns_dest_path() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().join("base.ext4");
    let dest = dir.path().join("vm-001.ext4");

    std::fs::write(&base, b"payload").unwrap();

    let rootfs = Rootfs::clone(&base, &dest).unwrap();
    assert_eq!(rootfs.path(), dest.as_path());
}

#[test]
fn new_at_wraps_existing_path_without_copy() {
    let dir = tempfile::tempdir().unwrap();
    let existing = dir.path().join("existing.ext4");
    std::fs::write(&existing, b"data").unwrap();

    let rootfs = Rootfs::new_at(&existing);
    assert_eq!(rootfs.path(), existing.as_path());
    // Base file must not have been touched.
    assert_eq!(std::fs::read(&existing).unwrap(), b"data");
}

#[test]
fn clone_missing_base_returns_io_error() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().join("nonexistent.ext4");
    let dest = dir.path().join("dest.ext4");

    let err = Rootfs::clone(&base, &dest).unwrap_err();
    match err {
        m80_storage::StorageError::Io { path, source } => {
            assert_eq!(path, dest, "Io must carry the dest path");
            assert_eq!(source.kind(), std::io::ErrorKind::NotFound);
        }
        other => panic!("expected Io error, got {other:?}"),
    }
}
