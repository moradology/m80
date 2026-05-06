use std::os::unix::fs::symlink;

use m80_storage::{Scratch, StorageError};

#[test]
fn sizing_obeys_padding_and_alignment() {
    let mib = 1024 * 1024;

    assert_eq!(Scratch::recommended_size_for_used_bytes(0), 64 * mib);
    assert_eq!(Scratch::recommended_size_for_used_bytes(20 * mib), 64 * mib);
    assert_eq!(Scratch::recommended_size_for_used_bytes(33 * mib), 68 * mib);
    assert_eq!(
        Scratch::recommended_size_for_used_bytes(96 * mib + 1),
        132 * mib
    );
}

#[test]
fn workspace_sizing_counts_regular_file_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = dir.path().join("workspace");
    let nested = workspace.join("nested");
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::write(workspace.join("a.bin"), vec![0_u8; 33 * 1024 * 1024]).unwrap();
    std::fs::write(nested.join("b.bin"), b"x").unwrap();

    assert_eq!(
        Scratch::recommended_size_for_workspace(&workspace).unwrap(),
        68 * 1024 * 1024
    );
}

#[test]
fn workspace_sizing_rejects_symlink_like_hydration() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = dir.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    std::fs::write(workspace.join("ok.txt"), b"ok").unwrap();
    symlink("ok.txt", workspace.join("link.txt")).unwrap();

    let err = Scratch::recommended_size_for_workspace(&workspace).unwrap_err();
    assert!(matches!(err, StorageError::AdmissibilityRefused));
}

#[test]
#[ignore = "requires root and a loop device"]
fn hydrates_from_host_workspace() {
    if !crate::common::require_root("hydrates_from_host_workspace") {
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    let workspace = dir.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    std::fs::write(workspace.join("hello.txt"), b"hello").unwrap();
    let nested = workspace.join("nested");
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::write(nested.join("world.txt"), b"world").unwrap();

    let image = dir.path().join("scratch.ext4");
    let scratch = Scratch::create(
        &workspace,
        &image,
        Scratch::recommended_size_for_workspace(&workspace).unwrap(),
    )
    .expect("create must hydrate the scratch image");

    assert_eq!(scratch.path(), image.as_path());
    assert!(image.exists());
}
