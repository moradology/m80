//! `recover_stale_run_root` on an empty run-root returns `Ok` with no work done.

use std::sync::Arc;

use m80_firecracker::{Backend, BackendConfig, CgroupMode};
use tempfile::TempDir;

fn fake_manifest() -> m80_image_manifest::Manifest {
    m80_image_manifest::Manifest {
        schema_version: 1,
        expected_firecracker_version: "v1.0.0".into(),
        kernel_image: std::path::PathBuf::from("/dev/null"),
        kernel_image_sha256: "0".repeat(64),
        output_rootfs_image: std::path::PathBuf::from("/dev/null"),
        output_rootfs_sha256: "0".repeat(64),
        source_rootfs_image: std::path::PathBuf::from("/dev/null"),
        source_rootfs_sha256: "0".repeat(64),
        daemon_binary_path: std::path::PathBuf::from("/dev/null"),
        daemon_binary_sha256: "0".repeat(64),
        service_unit_path: std::path::PathBuf::from("/dev/null"),
        service_unit_sha256: "0".repeat(64),
        workspace_mount_path: std::path::PathBuf::from("/dev/null"),
        workspace_mount_sha256: "0".repeat(64),
        boot_target: "multi-user.target".into(),
        guest_port: 9001,
        no_egress_reason: None,
        ready_marker: "GUESTD_READY".into(),
    }
}

#[test]
fn empty_run_root_returns_ok() {
    let dir = TempDir::new().expect("tempdir");
    let config = BackendConfig {
        discovery: m80_preflight::Discovery {
            firecracker_bin: std::path::PathBuf::from("/dev/null"),
            jailer_bin: std::path::PathBuf::from("/dev/null"),
            kernel: std::path::PathBuf::from("/dev/null"),
            rootfs: std::path::PathBuf::from("/dev/null"),
            manifest: fake_manifest(),
            run_root: dir.path().to_path_buf(),
            privilege: m80_preflight::PrivilegeStatus::Root,
            report: vec![],
        },
        max_concurrent_vms: 8,
        run_root: dir.path().to_path_buf(),
        jail_uid: 3000,
        jail_gid: 3000,
        cgroup_mode: CgroupMode::Disabled,
    };
    let backend = Arc::new(Backend::new(config).expect("Backend::new"));
    backend.recover_stale_run_root().expect("should succeed on empty run-root");
}

#[test]
fn nonexistent_run_root_returns_ok() {
    let config = BackendConfig {
        discovery: m80_preflight::Discovery {
            firecracker_bin: std::path::PathBuf::from("/dev/null"),
            jailer_bin: std::path::PathBuf::from("/dev/null"),
            kernel: std::path::PathBuf::from("/dev/null"),
            rootfs: std::path::PathBuf::from("/dev/null"),
            manifest: fake_manifest(),
            run_root: std::path::PathBuf::from("/tmp"),
            privilege: m80_preflight::PrivilegeStatus::Root,
            report: vec![],
        },
        max_concurrent_vms: 8,
        run_root: std::path::PathBuf::from("/nonexistent/path/xyz/m80-test"),
        jail_uid: 3000,
        jail_gid: 3000,
        cgroup_mode: CgroupMode::Disabled,
    };
    let backend = Arc::new(Backend::new(config).expect("Backend::new"));
    // A nonexistent run-root should return Ok immediately.
    backend
        .recover_stale_run_root()
        .expect("nonexistent run-root should return Ok");
}
