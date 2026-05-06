use std::path::Path;
use std::sync::Arc;

use m80_firecracker::{Backend, BackendConfig, CgroupMode};
use tempfile::TempDir;

use crate::common;

fn make_backend(run_root: &Path) -> Arc<Backend> {
    let config = BackendConfig {
        discovery: common::fake_discovery(run_root),
        max_concurrent_vms: 8,
        run_root: run_root.to_path_buf(),
        jail_uid: 3000,
        jail_gid: 3000,
        cgroup_mode: CgroupMode::Disabled,
    };
    Arc::new(Backend::new(config).expect("Backend::new"))
}

#[test]
fn repeat_calls_succeed() {
    let dir = TempDir::new().expect("tempdir");
    let orphan = dir.path().join("vm-orphan");
    std::fs::create_dir_all(orphan.join("nested")).unwrap();

    let backend = make_backend(dir.path());
    backend.recover_stale_run_root().unwrap();
    backend.recover_stale_run_root().unwrap();

    assert!(!orphan.exists());
}

#[test]
fn leaves_unowned_residue_alone() {
    let dir = TempDir::new().expect("tempdir");
    let preserved = dir.path().join(".preserved").join("triage-vm");
    std::fs::create_dir_all(&preserved).unwrap();
    std::fs::write(preserved.join("console.log"), b"guest stderr").unwrap();

    make_backend(dir.path()).recover_stale_run_root().unwrap();

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

    make_backend(dir.path()).recover_stale_run_root().unwrap();

    assert!(!orphan.exists());
}
