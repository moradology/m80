//! `recover_stale_run_root` on an empty run-root returns `Ok` with no work done.

mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use m80_firecracker::{Backend, BackendConfig, CgroupMode};
use tempfile::TempDir;

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
fn empty_run_root_returns_ok() {
    let dir = TempDir::new().expect("tempdir");
    make_backend(dir.path())
        .recover_stale_run_root()
        .expect("should succeed on empty run-root");
}

#[test]
fn nonexistent_run_root_returns_ok() {
    let dir = PathBuf::from("/nonexistent/path/xyz/m80-test");
    make_backend(&dir)
        .recover_stale_run_root()
        .expect("nonexistent run-root should return Ok");
}
