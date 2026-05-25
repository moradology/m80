mod common;

use m80_storage::Scratch;

#[test]
#[ignore = "requires-root requires-loop-device"]
fn hydrates_from_host_workspace() {
    if !common::require_root("hydrates_from_host_workspace") {
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
    let scratch = Scratch::create(&workspace, &image, 64 * 1024 * 1024)
        .expect("create must hydrate the scratch image");

    assert_eq!(scratch.path(), image.as_path());
    assert!(image.exists());
}
