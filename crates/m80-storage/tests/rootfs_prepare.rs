//! Tests for `Rootfs::prepare` and `Rootfs::new_at`.

use std::os::unix::fs::MetadataExt as _;

use m80_storage::{Rootfs, StorageError};

fn paths() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let run_dir = dir.path().join("vm-1");
    std::fs::create_dir(&run_dir).unwrap();
    let base = dir.path().join("base.ext4");
    let overlay = run_dir.join("rootfs.overlay.ext4");
    std::fs::write(&base, b"fake-base").unwrap();
    (dir, base, overlay)
}

/// `prepare` creates the overlay file at the specified path.
#[test]
fn prepare_creates_overlay_file() {
    let (_dir, base, overlay) = paths();

    Rootfs::prepare(&base, &overlay, 64 * 1024 * 1024).expect("prepare must succeed");

    assert!(overlay.exists(), "overlay file must be created");
}

/// `prepare` returns a `Rootfs` whose `base_path` is the caller-supplied base.
#[test]
fn prepare_base_path_matches_caller_supplied_base() {
    let (_dir, base, overlay) = paths();

    let rootfs = Rootfs::prepare(&base, &overlay, 64 * 1024 * 1024).unwrap();

    assert_eq!(rootfs.base_path(), base.as_path());
}

/// `prepare` returns a `Rootfs` whose `overlay_path` is the new overlay.
#[test]
fn prepare_overlay_path_matches_dest() {
    let (_dir, base, overlay) = paths();

    let rootfs = Rootfs::prepare(&base, &overlay, 64 * 1024 * 1024).unwrap();

    assert_eq!(rootfs.overlay_path(), overlay.as_path());
}

/// The sparse overlay file has exactly the requested size.
#[test]
fn prepare_overlay_file_has_correct_size() {
    let (_dir, base, overlay) = paths();

    let size: u64 = 64 * 1024 * 1024; // 64 MiB
    Rootfs::prepare(&base, &overlay, size).unwrap();

    let meta = std::fs::metadata(&overlay).unwrap();
    assert_eq!(
        meta.len(),
        size,
        "overlay file must report the requested size"
    );
}

#[test]
fn overlay_template_and_clone_are_sparse() {
    let (dir, base, overlay) = paths();
    let size: u64 = 64 * 1024 * 1024;

    Rootfs::prepare(&base, &overlay, size).unwrap();

    let template = dir.path().join(".rootfs-overlay-template-v1-67108864.ext4");
    assert_sparse_file(&template, size);
    assert_sparse_file(&overlay, size);
}

/// Missing parent directory returns `OverlayTemplateCloneFailed`.
#[test]
fn prepare_missing_parent_returns_overlay_template_clone_failed() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().join("base.ext4");
    std::fs::write(&base, b"fake-base").unwrap();

    // The parent "nonexistent/" does not exist.
    let overlay = dir.path().join("nonexistent").join("overlay.ext4");

    let err = Rootfs::prepare(&base, &overlay, 64 * 1024 * 1024).unwrap_err();
    match err {
        StorageError::OverlayTemplateCloneFailed { dest, .. } => {
            assert_eq!(dest, overlay, "error must carry the overlay path");
        }
        other => panic!("expected OverlayTemplateCloneFailed, got {other:?}"),
    }
}

/// `mkfs.ext4` on a zero-byte template exits non-zero; that surfaces as `SubprocessFailed`.
#[test]
fn prepare_mkfs_failure_returns_subprocess_failed() {
    let (_dir, base, overlay) = paths();
    let err = Rootfs::prepare(&base, &overlay, 0).unwrap_err();
    match err {
        StorageError::SubprocessFailed {
            program, status, ..
        } => {
            assert_eq!(program, "mkfs.ext4", "program must be mkfs.ext4");
            assert_ne!(status, "0", "mkfs must have exited non-zero");
        }
        other => panic!("expected SubprocessFailed, got {other:?}"),
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

#[test]
fn prepare_creates_template_metadata_once_and_reuses_it() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().join("base.ext4");
    std::fs::write(&base, b"fake-base").unwrap();
    let run1 = dir.path().join("vm-1");
    let run2 = dir.path().join("vm-2");
    std::fs::create_dir(&run1).unwrap();
    std::fs::create_dir(&run2).unwrap();
    let overlay1 = run1.join("rootfs.overlay.ext4");
    let overlay2 = run2.join("rootfs.overlay.ext4");

    Rootfs::prepare(&base, &overlay1, 64 * 1024 * 1024).unwrap();
    Rootfs::prepare(&base, &overlay2, 64 * 1024 * 1024).unwrap();

    let template = dir.path().join(".rootfs-overlay-template-v1-67108864.ext4");
    let meta = dir.path().join(".rootfs-overlay-template-v1-67108864.meta");
    assert!(template.exists());
    assert!(meta.exists());
    assert!(overlay1.exists());
    assert!(overlay2.exists());
}

#[test]
fn stale_template_metadata_is_a_hard_error() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().join("base.ext4");
    std::fs::write(&base, b"fake-base").unwrap();
    let run1 = dir.path().join("vm-1");
    let run2 = dir.path().join("vm-2");
    std::fs::create_dir(&run1).unwrap();
    std::fs::create_dir(&run2).unwrap();
    let overlay1 = run1.join("rootfs.overlay.ext4");
    let overlay2 = run2.join("rootfs.overlay.ext4");

    Rootfs::prepare(&base, &overlay1, 64 * 1024 * 1024).unwrap();
    let meta = dir.path().join(".rootfs-overlay-template-v1-67108864.meta");
    std::fs::write(&meta, "schema_version=1\nfs=ext4\nsize_bytes=123\n").unwrap();

    let err = Rootfs::prepare(&base, &overlay2, 64 * 1024 * 1024).unwrap_err();
    match err {
        StorageError::OverlayTemplateMismatch { path, .. } => assert_eq!(path, meta),
        other => panic!("expected OverlayTemplateMismatch, got {other:?}"),
    }
}

fn assert_sparse_file(path: &std::path::Path, apparent_size: u64) {
    let meta = std::fs::metadata(path).unwrap();
    assert_eq!(
        meta.len(),
        apparent_size,
        "{} apparent size",
        path.display()
    );
    let allocated = meta.blocks() * 512;
    assert!(
        allocated < apparent_size / 2,
        "{} must be sparse: allocated {allocated} bytes for apparent size {apparent_size}",
        path.display()
    );
}
