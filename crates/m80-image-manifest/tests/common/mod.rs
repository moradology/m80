//! Shared test helpers for `m80-image-manifest` integration tests.
//!
//! Each `tests/*.rs` file is its own integration crate, so we share via
//! `mod common;` rather than via a `pub use`.

use std::path::Path;

use m80_image_manifest::{Manifest, SCHEMA_VERSION};
use sha2::{Digest, Sha256};

/// Write six dummy artifact files into `dir` and return a `Manifest` whose
/// path + sha256 fields point at them. Tests that care about a specific
/// boot field (e.g., `guest_port`) can mutate the returned manifest before
/// writing it.
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
        boot_target: "multi-user.target".into(),
        daemon_binary_path: dir.join("guestd"),
        daemon_binary_sha256: hex::encode(Sha256::digest(b"guestd")),
        expected_firecracker_version: "v1.15.1".into(),
        guest_port: 8080,
        kernel_image: dir.join("vmlinux"),
        kernel_image_sha256: hex::encode(Sha256::digest(b"vmlinux")),
        no_egress_reason: None,
        output_rootfs_image: dir.join("output.ext4"),
        output_rootfs_sha256: hex::encode(Sha256::digest(b"output.ext4")),
        ready_marker: "READY".into(),
        schema_version: SCHEMA_VERSION,
        service_unit_path: dir.join("guestd.service"),
        service_unit_sha256: hex::encode(Sha256::digest(b"guestd.service")),
        source_rootfs_image: dir.join("source.ext4"),
        source_rootfs_sha256: hex::encode(Sha256::digest(b"source.ext4")),
        workspace_mount_path: dir.join("workspace.mount"),
        workspace_mount_sha256: hex::encode(Sha256::digest(b"workspace.mount")),
    }
}
