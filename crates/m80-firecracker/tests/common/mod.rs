//! Shared test fixtures: fake `Manifest` + fake `Discovery` so each
//! integration test doesn't reinvent the placeholder values.

use std::path::{Path, PathBuf};

pub fn fake_manifest() -> m80_image_manifest::Manifest {
    m80_image_manifest::Manifest {
        schema_version: 1,
        expected_firecracker_version: "v1.0.0".into(),
        kernel_image: PathBuf::from("/dev/null"),
        kernel_image_sha256: "0".repeat(64),
        output_rootfs_image: PathBuf::from("/dev/null"),
        output_rootfs_sha256: "0".repeat(64),
        source_rootfs_image: PathBuf::from("/dev/null"),
        source_rootfs_sha256: "0".repeat(64),
        daemon_binary_path: PathBuf::from("/dev/null"),
        daemon_binary_sha256: "0".repeat(64),
        service_unit_path: PathBuf::from("/dev/null"),
        service_unit_sha256: "0".repeat(64),
        workspace_mount_path: PathBuf::from("/dev/null"),
        workspace_mount_sha256: "0".repeat(64),
        boot_target: "multi-user.target".into(),
        guest_port: 9001,
        no_egress_reason: None,
        ready_marker: "GUESTD_READY".into(),
    }
}

pub fn fake_discovery(run_root: &Path) -> m80_preflight::Discovery {
    m80_preflight::Discovery {
        firecracker_bin: PathBuf::from("/dev/null"),
        jailer_bin: PathBuf::from("/dev/null"),
        kernel: PathBuf::from("/dev/null"),
        rootfs: PathBuf::from("/dev/null"),
        manifest: fake_manifest(),
        run_root: run_root.to_path_buf(),
        privilege: m80_preflight::PrivilegeStatus::Root,
        report: vec![],
    }
}
