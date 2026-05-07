//! Fake `Manifest` and `Discovery` fixtures for integration tests.
//!
//! Both `fake_manifest` and `fake_discovery_at` produce minimal placeholder
//! values — no real files are required.

use std::path::{Path, PathBuf};

/// Return a minimal [`m80_image_manifest::Manifest`] whose fields are
/// all synthetic `/dev/null` or repeated-char sha256 placeholders.
pub fn fake_manifest() -> m80_image_manifest::Manifest {
    m80_image_manifest::Manifest {
        boot_target: None,
        daemon_binary_path: "/tmp/m80-guestd".into(),
        daemon_binary_sha256: "0".repeat(64),
        expected_firecracker_version: "v1.0.0".to_owned(),
        guest_port: 52,
        image_kind: m80_image_manifest::ImageKind::Minimal,
        kernel_image: "/tmp/vmlinux".into(),
        kernel_image_sha256: "1".repeat(64),
        kernel_kind: m80_image_manifest::KernelKind::Stock,
        no_egress_reason: None,
        output_rootfs_image: "/tmp/rootfs.ext4".into(),
        output_rootfs_sha256: "2".repeat(64),
        ready_marker: "M80_READY".to_owned(),
        schema_version: m80_image_manifest::SCHEMA_VERSION,
        service_unit_path: None,
        service_unit_sha256: None,
        source_rootfs_image: None,
        source_rootfs_sha256: None,
        workspace_mount_path: None,
        workspace_mount_sha256: None,
    }
}

/// Return a [`m80_preflight::Discovery`] whose `run_root` is `run_root` and
/// whose other fields are synthetic placeholders.
pub fn fake_discovery_at(run_root: &Path) -> m80_preflight::Discovery {
    m80_preflight::Discovery {
        firecracker_bin: PathBuf::from("/tmp/firecracker"),
        jailer_bin: PathBuf::from("/tmp/jailer"),
        kernel: PathBuf::from("/tmp/vmlinux"),
        rootfs: PathBuf::from("/tmp/rootfs.ext4"),
        manifest: fake_manifest(),
        run_root: run_root.to_path_buf(),
        privilege: m80_preflight::PrivilegeStatus::Root,
        report: Vec::new(),
    }
}
