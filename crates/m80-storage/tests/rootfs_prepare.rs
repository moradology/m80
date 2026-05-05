//! Tests for `Rootfs::prepare` and `Rootfs::new_at`.

use m80_storage::{Rootfs, StorageError};

/// `prepare` creates the overlay file at the specified path.
#[test]
fn prepare_creates_overlay_file() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().join("base.ext4");
    let overlay = dir.path().join("rootfs.overlay.ext4");

    // Base must exist (prepare does not verify it, but it records the path).
    std::fs::write(&base, b"fake-base").unwrap();

    Rootfs::prepare(&base, &overlay, 64 * 1024 * 1024).expect("prepare must succeed");

    assert!(overlay.exists(), "overlay file must be created");
}

/// `prepare` returns a `Rootfs` whose `base_path` is the caller-supplied base.
#[test]
fn prepare_base_path_matches_caller_supplied_base() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().join("base.ext4");
    let overlay = dir.path().join("rootfs.overlay.ext4");
    std::fs::write(&base, b"fake-base").unwrap();

    let rootfs = Rootfs::prepare(&base, &overlay, 64 * 1024 * 1024).unwrap();

    assert_eq!(rootfs.base_path(), base.as_path());
}

/// `prepare` returns a `Rootfs` whose `overlay_path` is the new overlay.
#[test]
fn prepare_overlay_path_matches_dest() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().join("base.ext4");
    let overlay = dir.path().join("rootfs.overlay.ext4");
    std::fs::write(&base, b"fake-base").unwrap();

    let rootfs = Rootfs::prepare(&base, &overlay, 64 * 1024 * 1024).unwrap();

    assert_eq!(rootfs.overlay_path(), overlay.as_path());
}

/// The sparse overlay file has exactly the requested size.
#[test]
fn prepare_overlay_file_has_correct_size() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().join("base.ext4");
    let overlay = dir.path().join("rootfs.overlay.ext4");
    std::fs::write(&base, b"fake-base").unwrap();

    let size: u64 = 64 * 1024 * 1024; // 64 MiB
    Rootfs::prepare(&base, &overlay, size).unwrap();

    let meta = std::fs::metadata(&overlay).unwrap();
    assert_eq!(
        meta.len(),
        size,
        "overlay file must report the requested size"
    );
}

/// Missing parent directory returns `OverlayCreateFailed`.
#[test]
fn prepare_missing_parent_returns_overlay_create_failed() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().join("base.ext4");
    std::fs::write(&base, b"fake-base").unwrap();

    // The parent "nonexistent/" does not exist.
    let overlay = dir.path().join("nonexistent").join("overlay.ext4");

    let err = Rootfs::prepare(&base, &overlay, 64 * 1024 * 1024).unwrap_err();
    match err {
        StorageError::OverlayCreateFailed { path, .. } => {
            assert_eq!(path, overlay, "error must carry the overlay path");
        }
        other => panic!("expected OverlayCreateFailed, got {other:?}"),
    }
}

/// `mkfs.ext4` on a zero-byte file exits non-zero; that surfaces as `MkfsFailed`.
#[test]
fn prepare_mkfs_failure_returns_mkfs_failed() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().join("base.ext4");
    std::fs::write(&base, b"fake-base").unwrap();

    // size=0 causes mkfs.ext4 to reject the image.
    let overlay = dir.path().join("overlay.ext4");
    let err = Rootfs::prepare(&base, &overlay, 0).unwrap_err();
    match err {
        StorageError::MkfsFailed { path, status, .. } => {
            assert_eq!(path, overlay);
            assert_ne!(status, 0, "mkfs must have exited non-zero");
        }
        other => panic!("expected MkfsFailed, got {other:?}"),
    }
}

/// `new_at` wraps an existing `(base, overlay)` pair without any I/O.
#[test]
fn new_at_wraps_paths_without_io() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().join("base.ext4");
    let overlay = dir.path().join("overlay.ext4");

    // Files do NOT need to exist for new_at.
    let rootfs = Rootfs::new_at(&base, &overlay);

    assert_eq!(rootfs.base_path(), base.as_path());
    assert_eq!(rootfs.overlay_path(), overlay.as_path());
}
