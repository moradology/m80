//! Integration tests for `Scratch::extract` that require root + loop device.
//!
//! All tests are `#[ignore]`; run with:
//!   sudo cargo test -p m80-storage --test scratch_extract_real -- --ignored

mod common;

use std::os::unix::fs::PermissionsExt as _;

use m80_storage::Scratch;

/// Full create → extract round trip.
///
/// Creates a workspace, formats + hydrates a scratch image, then extracts it
/// back and verifies the ChangeSet matches.
#[test]
#[ignore = "requires root and a loop device"]
fn scratch_extract_round_trips_workspace() {
    if !common::require_root("scratch_extract_real") {
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    let workspace = dir.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    std::fs::write(workspace.join("a.txt"), b"alpha").unwrap();
    std::fs::write(workspace.join("b.txt"), b"beta").unwrap();

    let image = dir.path().join("scratch.ext4");
    Scratch::create(&workspace, &image, 64 * 1024 * 1024).expect("create");

    let into = dir.path().join("extracted");
    let cs = Scratch::extract(&image, &into, None).expect("extract");

    assert!(into.exists(), "into must exist after extract");
    assert!(cs.rejected.is_empty(), "no rejections expected");
    // staged contains files + the root directory itself is not listed.
    let staged_names: Vec<_> = cs
        .staged
        .iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect();
    assert!(
        staged_names.contains(&"a.txt".to_owned()),
        "a.txt must be staged: {staged_names:?}"
    );
    assert!(
        staged_names.contains(&"b.txt".to_owned()),
        "b.txt must be staged: {staged_names:?}"
    );
    assert!(cs.total_bytes > 0, "total_bytes must be non-zero");
}

#[test]
#[ignore = "requires root and a loop device"]
fn scratch_extract_rejects_into_already_exists() {
    if !common::require_root("scratch_extract_real") {
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    let workspace = dir.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();

    let image = dir.path().join("scratch.ext4");
    Scratch::create(&workspace, &image, 64 * 1024 * 1024).expect("create");

    // Create into before extract — must fail with SwapFailed.
    let into = dir.path().join("already_exists");
    std::fs::create_dir_all(&into).unwrap();

    let err = Scratch::extract(&image, &into, None).unwrap_err();
    assert!(
        matches!(err, m80_storage::StorageError::Io { .. }),
        "expected Io(AlreadyExists), got {err:?}"
    );
}

#[test]
#[ignore = "requires root and a loop device"]
fn writeback_preserves_file_modes() {
    if !common::require_root("writeback_preserves_file_modes") {
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    let workspace = dir.path().join("workspace");
    let private_dir = workspace.join("private-dir");
    std::fs::create_dir_all(&private_dir).unwrap();
    set_mode(&private_dir, 0o700);
    write_with_mode(&workspace.join("regular.txt"), b"regular", 0o644);
    write_with_mode(&workspace.join("readonly.txt"), b"readonly", 0o400);
    write_with_mode(&workspace.join("executable.sh"), b"#!/bin/sh\n", 0o755);

    let image = dir.path().join("scratch.ext4");
    Scratch::create(&workspace, &image, 64 * 1024 * 1024).expect("create");

    let into = dir.path().join("extracted");
    Scratch::extract(&image, &into, None).expect("extract");

    assert_mode(&into.join("private-dir"), 0o700);
    assert_mode(&into.join("regular.txt"), 0o644);
    assert_mode(&into.join("readonly.txt"), 0o400);
    assert_mode(&into.join("executable.sh"), 0o755);
}

fn write_with_mode(path: &std::path::Path, bytes: &[u8], mode: u32) {
    std::fs::write(path, bytes).unwrap();
    set_mode(path, mode);
}

fn set_mode(path: &std::path::Path, mode: u32) {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
}

fn assert_mode(path: &std::path::Path, expected: u32) {
    let actual = std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
    assert_eq!(actual, expected, "{} mode", path.display());
}
