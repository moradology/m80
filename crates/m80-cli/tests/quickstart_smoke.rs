//! Smoke test for `m80 quickstart --no-run` with a local release-like tarball.

mod common;

use common::m80;

use std::process::Command as StdCommand;

use m80_image_manifest::{ImageKind, KernelKind, Manifest, SCHEMA_VERSION};
use serde_json::Value;

fn run_checked(cmd: &mut StdCommand, label: &str) {
    let output = cmd.output().unwrap();
    assert!(
        output.status.success(),
        "{label} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn output_checked(cmd: &mut StdCommand, label: &str) -> std::process::Output {
    let output = cmd.output().unwrap();
    assert!(
        output.status.success(),
        "{label} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn write_release_tarball(dir: &tempfile::TempDir) -> std::path::PathBuf {
    let src = dir.path().join("src");
    std::fs::create_dir(&src).unwrap();
    std::fs::write(src.join("vmlinux"), b"kernel").unwrap();
    std::fs::write(src.join("output.ext4"), b"rootfs").unwrap();
    std::fs::write(src.join("m80-guestd"), b"guestd").unwrap();
    write_manifest_with_stale_paths(&src);

    let sums = output_checked(
        StdCommand::new("sha256sum")
            .args([
                "vmlinux",
                "output.ext4",
                "output.ext4.manifest.json",
                "m80-guestd",
            ])
            .current_dir(&src),
        "sha256sum",
    );
    std::fs::write(src.join("SHA256SUMS"), sums.stdout).unwrap();

    let tarball = dir.path().join("m80-artifacts.tar.gz");
    run_checked(
        StdCommand::new("tar")
            .arg("-czf")
            .arg(&tarball)
            .arg("-C")
            .arg(&src)
            .arg("."),
        "tar",
    );
    tarball
}

fn write_manifest_with_stale_paths(src: &std::path::Path) {
    let stale = std::path::PathBuf::from("/tmp/m80-release-artifacts");
    let manifest = Manifest {
        boot_target: None,
        daemon_binary_path: stale.join("m80-guestd"),
        daemon_binary_sha256: sha256_hex(&src.join("m80-guestd")),
        expected_firecracker_version: "v1.15.1".to_owned(),
        guest_port: m80_proto::GUEST_PORT_DEFAULT,
        image_kind: ImageKind::Minimal,
        kernel_image: stale.join("vmlinux"),
        kernel_image_sha256: sha256_hex(&src.join("vmlinux")),
        kernel_kind: KernelKind::Stock,
        no_egress_reason: Some(m80_image_manifest::DEFAULT_NO_EGRESS_REASON.to_owned()),
        output_rootfs_image: stale.join("output.ext4"),
        output_rootfs_sha256: sha256_hex(&src.join("output.ext4")),
        ready_marker: m80_proto::READY_MARKER_DEFAULT.to_owned(),
        schema_version: SCHEMA_VERSION,
        service_unit_path: None,
        service_unit_sha256: None,
        source_rootfs_image: None,
        source_rootfs_sha256: None,
        workspace_mount_path: None,
        workspace_mount_sha256: None,
    };
    manifest
        .write(&src.join("output.ext4.manifest.json"))
        .unwrap();
}

fn sha256_hex(path: &std::path::Path) -> String {
    let output = output_checked(StdCommand::new("sha256sum").arg(path), "sha256sum");
    String::from_utf8(output.stdout)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .to_owned()
}

#[test]
fn quickstart_no_run_installs_verified_artifacts() {
    let dir = tempfile::tempdir().unwrap();
    let tarball = write_release_tarball(&dir);
    let dst = dir.path().join("dst");
    let run_root = dir.path().join("run");

    m80()
        .args([
            "quickstart",
            "--artifact-url",
            &format!("file://{}", tarball.display()),
            "--artifact-dir",
            dst.to_str().unwrap(),
            "--run-root",
            run_root.to_str().unwrap(),
            "--no-run",
        ])
        .assert()
        .success();

    for artifact in [
        "vmlinux",
        "output.ext4",
        "output.ext4.manifest.json",
        "m80-guestd",
    ] {
        assert!(
            dst.join(artifact).is_file(),
            "quickstart should install {artifact}"
        );
    }
    assert!(run_root.is_dir(), "quickstart should create run-root");

    let manifest = Manifest::read(&dst.join("output.ext4.manifest.json")).unwrap();
    assert_eq!(manifest.kernel_image, dst.join("vmlinux"));
    assert_eq!(manifest.output_rootfs_image, dst.join("output.ext4"));
    assert_eq!(manifest.daemon_binary_path, dst.join("m80-guestd"));
    manifest.verify(&dst).unwrap();
}

#[test]
fn quickstart_json_no_run_keeps_stdout_machine_readable() {
    let dir = tempfile::tempdir().unwrap();
    let tarball = write_release_tarball(&dir);
    let dst = dir.path().join("json-dst");
    let run_root = dir.path().join("json-run");

    let output = m80()
        .args([
            "--json",
            "quickstart",
            "--artifact-url",
            &format!("file://{}", tarball.display()),
            "--artifact-dir",
            dst.to_str().unwrap(),
            "--run-root",
            run_root.to_str().unwrap(),
            "--no-run",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "quickstart --json failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["version"], 1);
    assert_eq!(value["data"]["ran_probe"], false);
    assert_eq!(
        value["data"]["artifact_dir"].as_str(),
        Some(dst.to_str().unwrap())
    );
}

#[test]
fn quickstart_json_requires_no_run_to_keep_stdout_machine_readable() {
    let output = m80()
        .args([
            "--json",
            "quickstart",
            "--artifact-url",
            "file:///tmp/m80-artifacts.tar.gz",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        output.stdout.is_empty(),
        "JSON quickstart config error should not write stdout"
    );

    let value: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(value["version"], 1);
    assert_eq!(value["data"]["variant"], "Config");
    assert!(
        value["data"]["detail"]
            .as_str()
            .unwrap()
            .contains("--json requires --no-run"),
        "unexpected error payload: {value}"
    );
}
