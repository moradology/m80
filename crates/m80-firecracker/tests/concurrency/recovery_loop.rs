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
fn no_background_recovery_task_is_spawned_by_backend_new() {
    let dir = TempDir::new().unwrap();
    let orphan = dir.path().join("vm-orphan");
    std::fs::create_dir_all(&orphan).unwrap();

    let _backend = make_backend(dir.path());

    assert!(orphan.exists());
}

#[test]
fn recovery_interval_is_not_an_orchestrator_constant_in_v0_1() {
    let _: fn(&Backend) -> Result<(), m80_firecracker::FcError> = Backend::recover_stale_run_root;
}

#[test]
fn recovery_is_synchronous_explicit_call() {
    let dir = TempDir::new().unwrap();
    let orphan = dir.path().join("vm-orphan");
    std::fs::create_dir_all(&orphan).unwrap();
    let backend = make_backend(dir.path());

    backend.recover_stale_run_root().unwrap();

    assert!(!orphan.exists());
}

#[test]
fn startup_recovery_is_caller_driven_before_first_admission() {
    let dir = TempDir::new().unwrap();
    let orphan = dir.path().join("vm-orphan");
    std::fs::create_dir_all(&orphan).unwrap();
    let backend = make_backend(dir.path());

    assert!(orphan.exists());
    backend.recover_stale_run_root().unwrap();
    assert!(!orphan.exists());
}
