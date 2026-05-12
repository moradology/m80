use std::path::Path;
use std::sync::Arc;

use m80_firecracker::{Backend, BackendConfig, CgroupMode, OWNERSHIP_LOCK};
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
fn ownership_lock_is_the_marker_file() {
    assert_eq!(OWNERSHIP_LOCK, "ownership.lock");
}

#[test]
fn current_process_ownership_lock_preserves_run_dir() {
    let dir = TempDir::new().unwrap();
    let live = dir.path().join("vm-live");
    std::fs::create_dir_all(&live).unwrap();
    std::fs::write(
        live.join(OWNERSHIP_LOCK),
        format!("pid={}\nstarted_at=1\n", std::process::id()),
    )
    .unwrap();

    make_backend(dir.path()).recover_stale_run_root(false).unwrap();

    assert!(live.exists());
}

#[test]
fn stale_detection_uses_ownership_and_jailer_state_not_socket_probes() {
    let dir = TempDir::new().unwrap();
    let stale = dir.path().join("vm-stale");
    std::fs::create_dir_all(&stale).unwrap();
    std::fs::write(stale.join("firecracker.sock"), b"not a socket").unwrap();
    std::fs::write(stale.join("vsock.sock"), b"not a socket").unwrap();

    make_backend(dir.path()).recover_stale_run_root(false).unwrap();

    assert!(!stale.exists());
}

#[test]
fn preserves_run_dir_when_ownership_lock_is_ambiguous() {
    let dir = TempDir::new().unwrap();
    let ambiguous = dir.path().join("vm-ambiguous");
    std::fs::create_dir_all(&ambiguous).unwrap();
    std::fs::write(ambiguous.join(OWNERSHIP_LOCK), b"not-parseable").unwrap();

    make_backend(dir.path()).recover_stale_run_root(false).unwrap();

    assert!(ambiguous.exists());
}
