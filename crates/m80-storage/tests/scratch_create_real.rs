//! Integration tests for `Scratch::create` that require root + loop device.
//!
//! All tests are `#[ignore]`; run with:
//!   sudo cargo test -p m80-storage --test scratch_create_real -- --ignored

mod common;

use m80_storage::Scratch;

/// Full create-and-verify round trip.
#[test]
#[ignore = "requires root and a loop device"]
fn scratch_create_hydrates_workspace() {
    if !common::require_root("scratch_create_real") {
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    let workspace = dir.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    std::fs::write(workspace.join("hello.txt"), b"hello").unwrap();
    let sub = workspace.join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(sub.join("world.txt"), b"world").unwrap();

    let image = dir.path().join("scratch.ext4");
    // 64 MiB is the minimum viable size for mkfs.ext4.
    let scratch =
        Scratch::create(&workspace, &image, 64 * 1024 * 1024).expect("create must succeed");
    assert_eq!(scratch.path(), image.as_path());
    assert!(image.exists());
}

#[test]
#[ignore = "requires root and a loop device"]
fn scratch_create_rejects_symlink_in_workspace() {
    use std::os::unix::fs::symlink;

    if !common::require_root("scratch_create_real") {
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    let workspace = dir.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    std::fs::write(workspace.join("ok.txt"), b"ok").unwrap();
    symlink("ok.txt", workspace.join("link.txt")).unwrap();

    let image = dir.path().join("scratch.ext4");
    let err = Scratch::create(&workspace, &image, 64 * 1024 * 1024).unwrap_err();
    assert!(
        matches!(err, m80_storage::StorageError::AdmissibilityRefused),
        "expected AdmissibilityRefused, got {err:?}"
    );
}
