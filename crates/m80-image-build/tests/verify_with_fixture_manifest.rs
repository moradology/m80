//! Verify subcommand integration test: build a fixture manifest with known
//! hashes, then invoke `m80-image-build verify --rootfs <path>` and assert
//! "verified" is printed.

use std::path::PathBuf;

use assert_cmd::Command;
use m80_image_manifest::{ImageKind, KernelKind, Manifest, SCHEMA_VERSION};
use sha2::{Digest, Sha256};
use tempfile::TempDir;

fn sha256_of(content: &[u8]) -> String {
    hex::encode(Sha256::digest(content))
}

/// Write six fixture artifact files and return a correctly-populated Manifest.
fn write_fixture_artifacts(dir: &TempDir) -> Manifest {
    let artifacts = [
        ("vmlinux", b"kernel-bytes" as &[u8]),
        ("source.ext4", b"source-rootfs-bytes"),
        ("output.ext4", b"output-rootfs-bytes"),
        ("m80-guestd", b"daemon-bytes"),
        ("m80-guestd.service", b"service-unit-bytes"),
        ("workspace.mount", b"workspace-mount-bytes"),
    ];

    for (name, content) in &artifacts {
        std::fs::write(dir.path().join(name), content).unwrap();
    }

    Manifest {
        boot_target: Some("multi-user.target".to_string()),
        daemon_binary_path: dir.path().join("m80-guestd"),
        daemon_binary_sha256: sha256_of(b"daemon-bytes"),
        expected_firecracker_version: "v1.15.1".to_string(),
        guest_port: 9001,
        image_kind: ImageKind::Ubuntu,
        kernel_image: dir.path().join("vmlinux"),
        kernel_image_sha256: sha256_of(b"kernel-bytes"),
        kernel_kind: KernelKind::Stock,
        no_egress_reason: None,
        output_rootfs_image: dir.path().join("output.ext4"),
        output_rootfs_sha256: sha256_of(b"output-rootfs-bytes"),
        ready_marker: "GUESTD_READY".to_string(),
        schema_version: SCHEMA_VERSION,
        service_unit_path: Some(dir.path().join("m80-guestd.service")),
        service_unit_sha256: Some(sha256_of(b"service-unit-bytes")),
        source_rootfs_image: Some(dir.path().join("source.ext4")),
        source_rootfs_sha256: Some(sha256_of(b"source-rootfs-bytes")),
        workspace_mount_path: Some(dir.path().join("workspace.mount")),
        workspace_mount_sha256: Some(sha256_of(b"workspace-mount-bytes")),
    }
}

#[test]
fn verify_prints_verified_on_correct_manifest() {
    let dir = tempfile::tempdir().unwrap();
    let manifest = write_fixture_artifacts(&dir);

    // Write manifest beside the rootfs.
    let rootfs_path = dir.path().join("output.ext4");
    let manifest_path = PathBuf::from(format!("{}.manifest.json", rootfs_path.display()));
    manifest.write(&manifest_path).unwrap();

    let mut cmd = Command::cargo_bin("m80-image-build").unwrap();
    cmd.args(["verify", "--rootfs", rootfs_path.to_str().unwrap()]);
    let output = cmd.output().unwrap();

    assert!(
        output.status.success(),
        "verify should exit 0; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.trim() == "verified",
        "expected 'verified' on stdout, got: {stdout:?}"
    );
}

#[test]
fn verify_fails_on_tampered_artifact() {
    let dir = tempfile::tempdir().unwrap();
    let manifest = write_fixture_artifacts(&dir);

    let rootfs_path = dir.path().join("output.ext4");
    let manifest_path = PathBuf::from(format!("{}.manifest.json", rootfs_path.display()));
    manifest.write(&manifest_path).unwrap();

    // Tamper with the kernel.
    std::fs::write(dir.path().join("vmlinux"), b"corrupted").unwrap();

    let mut cmd = Command::cargo_bin("m80-image-build").unwrap();
    cmd.args(["verify", "--rootfs", rootfs_path.to_str().unwrap()]);
    let output = cmd.output().unwrap();

    assert!(
        !output.status.success(),
        "verify should exit non-zero for tampered artifact"
    );
    // Pin which artifact failed so a future bug that makes verify pass
    // without checking the kernel sha would surface here.
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("kernel_image"),
        "expected 'kernel_image' in stderr, got: {stderr}"
    );
}
