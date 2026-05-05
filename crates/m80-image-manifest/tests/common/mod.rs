//! Shared test helpers for `m80-image-manifest` integration tests.
//!
//! Each `tests/*.rs` file is its own integration crate, so we share via
//! `mod common;` rather than via a `pub use`.

use std::path::Path;

use m80_image_manifest::{ImageKind, KernelKind, Manifest, SCHEMA_VERSION};
use sha2::{Digest, Sha256};

/// Write six dummy artifact files into `dir` and return a `Manifest` whose
/// path + sha256 fields point at them. The image_kind is `Ubuntu` so all
/// systemd / source-rootfs Options are populated.
pub fn make_artifacts(dir: &Path) -> Manifest {
    for name in &[
        "vmlinux",
        "source.ext4",
        "output.ext4",
        "guestd",
        "guestd.service",
        "workspace.mount",
    ] {
        std::fs::write(dir.join(name), name.as_bytes()).unwrap();
    }
    Manifest {
        boot_target: Some("multi-user.target".into()),
        daemon_binary_path: dir.join("guestd"),
        daemon_binary_sha256: hex::encode(Sha256::digest(b"guestd")),
        expected_firecracker_version: "v1.15.1".into(),
        guest_port: 8080,
        image_kind: ImageKind::Ubuntu,
        kernel_image: dir.join("vmlinux"),
        kernel_image_sha256: hex::encode(Sha256::digest(b"vmlinux")),
        kernel_kind: KernelKind::Stock,
        no_egress_reason: None,
        output_rootfs_image: dir.join("output.ext4"),
        output_rootfs_sha256: hex::encode(Sha256::digest(b"output.ext4")),
        ready_marker: "READY".into(),
        schema_version: SCHEMA_VERSION,
        service_unit_path: Some(dir.join("guestd.service")),
        service_unit_sha256: Some(hex::encode(Sha256::digest(b"guestd.service"))),
        source_rootfs_image: Some(dir.join("source.ext4")),
        source_rootfs_sha256: Some(hex::encode(Sha256::digest(b"source.ext4"))),
        workspace_mount_path: Some(dir.join("workspace.mount")),
        workspace_mount_sha256: Some(hex::encode(Sha256::digest(b"workspace.mount"))),
    }
}

/// Same as [`make_artifacts`] but for `ImageKind::Minimal` — only the
/// kernel, output rootfs, and daemon binary exist; all systemd /
/// source-rootfs Options are `None`.
///
/// Each `tests/*.rs` is its own crate via `mod common;`, so functions
/// not used by a particular test file are flagged dead from that
/// crate's perspective. Suppress at the function level.
#[allow(dead_code)]
pub fn make_minimal_artifacts(dir: &Path) -> Manifest {
    for name in &["vmlinux", "output.ext4", "guestd"] {
        std::fs::write(dir.join(name), name.as_bytes()).unwrap();
    }
    Manifest {
        boot_target: None,
        daemon_binary_path: dir.join("guestd"),
        daemon_binary_sha256: hex::encode(Sha256::digest(b"guestd")),
        expected_firecracker_version: "v1.15.1".into(),
        guest_port: 8080,
        image_kind: ImageKind::Minimal,
        kernel_image: dir.join("vmlinux"),
        kernel_image_sha256: hex::encode(Sha256::digest(b"vmlinux")),
        kernel_kind: KernelKind::Stock,
        no_egress_reason: None,
        output_rootfs_image: dir.join("output.ext4"),
        output_rootfs_sha256: hex::encode(Sha256::digest(b"output.ext4")),
        ready_marker: "READY".into(),
        schema_version: SCHEMA_VERSION,
        service_unit_path: None,
        service_unit_sha256: None,
        source_rootfs_image: None,
        source_rootfs_sha256: None,
        workspace_mount_path: None,
        workspace_mount_sha256: None,
    }
}
