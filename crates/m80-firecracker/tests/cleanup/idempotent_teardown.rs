use tempfile::TempDir;

use crate::common;

#[test]
fn repeat_calls_succeed() {
    let dir = TempDir::new().expect("tempdir");
    let orphan = dir.path().join("vm-orphan");
    std::fs::create_dir_all(orphan.join("nested")).unwrap();

    let backend = common::make_fake_backend(8, dir.path());
    backend.recover_stale_run_root(false).unwrap();
    backend.recover_stale_run_root(false).unwrap();

    assert!(!orphan.exists());
}

#[test]
fn leaves_unowned_residue_alone() {
    let dir = TempDir::new().expect("tempdir");
    let preserved = dir.path().join(".preserved").join("triage-vm");
    std::fs::create_dir_all(&preserved).unwrap();
    std::fs::write(preserved.join("console.log"), b"guest stderr").unwrap();

    common::make_fake_backend(8, dir.path())
        .recover_stale_run_root(false)
        .unwrap();

    assert!(preserved.exists());
    assert_eq!(
        std::fs::read(preserved.join("console.log")).unwrap(),
        b"guest stderr"
    );
}

#[test]
fn startup_scavenge_uses_same_path() {
    let dir = TempDir::new().expect("tempdir");
    let orphan = dir.path().join("vm-orphan");
    std::fs::create_dir_all(orphan.join("nested")).unwrap();
    std::fs::write(orphan.join("nested/state.txt"), b"state").unwrap();

    common::make_fake_backend(8, dir.path())
        .recover_stale_run_root(false)
        .unwrap();

    assert!(!orphan.exists());
}
