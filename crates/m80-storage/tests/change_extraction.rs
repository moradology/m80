mod common;

use m80_storage::{Scratch, StorageError};
use std::path::Path;

#[test]
fn extraction_only_when_requested() {
    let dir = tempfile::tempdir().unwrap();
    let image = dir.path().join("scratch.ext4");
    let host_workspace = dir.path().join("host-workspace");
    std::fs::write(&image, b"not an ext4 image").unwrap();
    std::fs::create_dir_all(&host_workspace).unwrap();
    std::fs::write(host_workspace.join("live.txt"), b"live").unwrap();

    let before = std::fs::read(host_workspace.join("live.txt")).unwrap();
    let image_before = std::fs::read(&image).unwrap();

    assert_eq!(before, b"live");
    assert_eq!(std::fs::read(&image).unwrap(), image_before);
}

#[test]
fn rollback_on_extract_failure() {
    let dir = tempfile::tempdir().unwrap();
    let image = dir.path().join("scratch.ext4");
    let into = dir.path().join("workspace");
    std::fs::write(&image, b"not an ext4 image").unwrap();
    std::fs::create_dir_all(&into).unwrap();
    std::fs::write(into.join("original.txt"), b"original").unwrap();

    let err = Scratch::extract(&image, &into, None).unwrap_err();

    assert!(matches!(err, StorageError::Io { .. }));
    assert_eq!(
        std::fs::read(into.join("original.txt")).unwrap(),
        b"original"
    );
}

#[test]
#[ignore = "requires root and a loop device"]
fn stages_into_sibling_directory() {
    if !common::require_root("stages_into_sibling_directory") {
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    let workspace = dir.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    std::fs::write(workspace.join("a.txt"), b"alpha").unwrap();

    let image = dir.path().join("scratch.ext4");
    Scratch::create(&workspace, &image, 64 * 1024 * 1024).expect("create");

    let into = dir.path().join("extracted");
    let change_set = Scratch::extract(&image, &into, None).expect("extract");

    assert!(into.exists());
    assert!(change_set
        .staged
        .iter()
        .any(|path| path == Path::new("a.txt")));
    let residue: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with(".extracted.m80-writeback-stage-"))
        .collect();
    assert!(residue.is_empty(), "stage residue left behind: {residue:?}");
}

#[test]
#[ignore = "requires root and a loop device"]
fn loop_mount_extracts_changed_file_set() {
    if !common::require_root("loop_mount_extracts_changed_file_set") {
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    let workspace = dir.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    std::fs::write(workspace.join("changed.txt"), b"changed").unwrap();

    let image = dir.path().join("scratch.ext4");
    Scratch::create(&workspace, &image, 64 * 1024 * 1024).expect("create");

    let into = dir.path().join("extracted");
    let change_set = Scratch::extract(&image, &into, None).expect("extract");

    assert!(change_set.rejected.is_empty());
    assert_eq!(std::fs::read(into.join("changed.txt")).unwrap(), b"changed");
}
