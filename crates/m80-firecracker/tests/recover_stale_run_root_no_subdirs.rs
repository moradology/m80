//! `recover_stale_run_root` on an empty run-root returns `Ok` with no work done.

mod common;

use std::path::PathBuf;

use tempfile::TempDir;

#[test]
fn empty_run_root_returns_ok() {
    let dir = TempDir::new().expect("tempdir");
    common::make_fake_backend(8, dir.path())
        .recover_stale_run_root(false)
        .expect("should succeed on empty run-root");
}

#[test]
fn nonexistent_run_root_returns_ok() {
    let dir = PathBuf::from("/nonexistent/path/xyz/m80-test");
    common::make_fake_backend(8, &dir)
        .recover_stale_run_root(false)
        .expect("nonexistent run-root should return Ok");
}

#[test]
fn orphan_subdir_without_jail_state_is_reaped() {
    let dir = TempDir::new().expect("tempdir");
    let orphan = dir.path().join("vm-orphan");
    std::fs::create_dir_all(orphan.join("nested")).unwrap();
    std::fs::write(orphan.join("nested/state.txt"), b"state").unwrap();

    common::make_fake_backend(8, dir.path())
        .recover_stale_run_root(false)
        .expect("orphan run-dir recovery should succeed");

    assert!(!orphan.exists());
}

#[test]
fn live_ownership_lock_preserves_run_dir() {
    let dir = TempDir::new().expect("tempdir");
    let live = dir.path().join("vm-live");
    std::fs::create_dir_all(&live).unwrap();
    let pid = std::process::id();
    std::fs::write(
        live.join("ownership.lock"),
        format!("pid={pid}\nstarted_at=1\n"),
    )
    .unwrap();

    common::make_fake_backend(8, dir.path())
        .recover_stale_run_root(false)
        .expect("live run-dir recovery should succeed");

    assert!(live.exists());
}
