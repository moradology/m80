//! Smoke test for `m80 quickstart --no-run` with a local release-like tarball.

mod common;

use common::m80;

use std::process::Command as StdCommand;

use m80_image_manifest::{
    BuildReceipt, BuildReceiptArtifact, BuildReceiptArtifactKind, ImageKind, InstallProvenance,
    InstallProvenanceArtifact, InstallProvenanceRewrite, KernelKind, Manifest, RootfsFormat,
};
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
    write_release_tarball_inner(dir, None)
}

fn write_release_tarball_with_bundled_host_manifest(dir: &tempfile::TempDir) -> std::path::PathBuf {
    write_release_tarball_inner(dir, Some("host-binaries.manifest.json"))
}

fn write_release_tarball_with_nested_bundled_host_manifest(
    dir: &tempfile::TempDir,
) -> std::path::PathBuf {
    write_release_tarball_inner(dir, Some("nested/host-binaries.manifest.json"))
}

fn write_release_tarball_inner(
    dir: &tempfile::TempDir,
    host_manifest_relpath: Option<&str>,
) -> std::path::PathBuf {
    let src = dir.path().join("src");
    std::fs::create_dir(&src).unwrap();
    std::fs::write(src.join("vmlinux"), b"kernel").unwrap();
    std::fs::write(src.join("output.ext4"), b"rootfs").unwrap();
    std::fs::write(src.join("m80-guestd"), b"guestd").unwrap();
    write_manifest_with_stale_paths(&src);
    write_build_receipt_with_stale_paths(&src);
    let mut checksum_inputs = vec![
        "vmlinux",
        "output.ext4",
        "output.ext4.manifest.json",
        "output.ext4.build-receipt.json",
        "m80-guestd",
    ];
    if let Some(relpath) = host_manifest_relpath {
        let host_manifest = src.join(relpath);
        std::fs::create_dir_all(host_manifest.parent().unwrap()).unwrap();
        std::fs::write(
            &host_manifest,
            br#"{
  "binaries": [],
  "launch_material": [
    {
      "name": "firecracker_seccomp_filter",
      "path": "/opt/firecracker/bin/firecracker-seccomp-filter.bin",
      "sha256": "0000000000000000000000000000000000000000000000000000000000000000",
      "version": "v1.15.1"
    }
  ],
  "schema_version": 4
}
"#,
        )
        .unwrap();
        checksum_inputs.push(relpath);
    }

    let sums = output_checked(
        StdCommand::new("sha256sum")
            .args(checksum_inputs)
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
    let tarball_sha = sha256_hex(&tarball);
    std::fs::write(
        format!("{}.sha256", tarball.display()),
        format!(
            "{tarball_sha}  {}\n",
            tarball.file_name().unwrap().to_string_lossy()
        ),
    )
    .unwrap();
    tarball
}

fn write_manifest_with_stale_paths(src: &std::path::Path) {
    let stale = std::path::PathBuf::from("/tmp/m80-release-artifacts");
    let manifest = Manifest::new(
        stale.join("m80-guestd"),
        sha256_hex(&src.join("m80-guestd")),
        "v1.15.1".to_owned(),
        m80_proto::GUEST_PORT_DEFAULT,
        ImageKind::Minimal,
        stale.join("vmlinux"),
        sha256_hex(&src.join("vmlinux")),
        KernelKind::Stock,
        Some(m80_image_manifest::DEFAULT_NO_EGRESS_REASON.to_owned()),
        stale.join("output.ext4"),
        sha256_hex(&src.join("output.ext4")),
        m80_proto::READY_MARKER_DEFAULT.to_owned(),
        RootfsFormat::Ext4,
        None,
        None,
    );
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
    let receipt = BuildReceipt::read(&dst.join("output.ext4.build-receipt.json")).unwrap();
    assert_eq!(receipt.manifest_path, dst.join("output.ext4.manifest.json"));
    assert_eq!(
        receipt.manifest_sha256,
        sha256_hex(&dst.join("output.ext4.manifest.json"))
    );
    let provenance = InstallProvenance::read(&dst.join("install-provenance.json")).unwrap();
    assert_eq!(provenance.release_tag, None);
    assert_eq!(provenance.transforms.len(), 2);
    assert_rewrite_record(
        &provenance,
        InstallProvenanceArtifact::GuestManifest,
        "output.ext4.manifest.json",
        &dst.join("output.ext4.manifest.json"),
    );
    assert_rewrite_record(
        &provenance,
        InstallProvenanceArtifact::BuildReceipt,
        "output.ext4.build-receipt.json",
        &dst.join("output.ext4.build-receipt.json"),
    );
    assert!(
        !dst.join("host-binaries.manifest.json").exists(),
        "quickstart must not install a bundled host-binaries manifest"
    );
}

fn assert_rewrite_record(
    provenance: &InstallProvenance,
    artifact: InstallProvenanceArtifact,
    source_name: &str,
    installed_path: &std::path::Path,
) {
    let transform = provenance
        .transforms
        .iter()
        .find(|transform| transform.artifact == artifact)
        .unwrap_or_else(|| panic!("missing provenance transform for {artifact:?}"));
    assert_eq!(transform.source_path, std::path::PathBuf::from(source_name));
    assert_eq!(transform.installed_path, installed_path);
    assert_eq!(
        transform.rewrite,
        InstallProvenanceRewrite::InstallPathRewrite
    );
    assert_eq!(transform.installed_sha256, sha256_hex(installed_path));
    assert_ne!(transform.source_sha256, transform.installed_sha256);
}

fn write_build_receipt_with_stale_paths(src: &std::path::Path) {
    let stale = std::path::PathBuf::from("/tmp/m80-release-artifacts");
    let receipt = BuildReceipt::new(
        stale.join("output.ext4.manifest.json"),
        sha256_hex(&src.join("output.ext4.manifest.json")),
        vec![
            BuildReceiptArtifact {
                kind: BuildReceiptArtifactKind::KernelImage,
                path: stale.join("vmlinux"),
                sha256: sha256_hex(&src.join("vmlinux")),
            },
            BuildReceiptArtifact {
                kind: BuildReceiptArtifactKind::OutputRootfsImage,
                path: stale.join("output.ext4"),
                sha256: sha256_hex(&src.join("output.ext4")),
            },
            BuildReceiptArtifact {
                kind: BuildReceiptArtifactKind::DaemonBinaryPath,
                path: stale.join("m80-guestd"),
                sha256: sha256_hex(&src.join("m80-guestd")),
            },
        ],
    );
    receipt
        .write(&src.join("output.ext4.build-receipt.json"))
        .unwrap();
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
fn quickstart_rejects_tarball_when_external_checksum_mismatches() {
    let dir = tempfile::tempdir().unwrap();
    let tarball = write_release_tarball(&dir);
    std::fs::write(
        format!("{}.sha256", tarball.display()),
        "0000000000000000000000000000000000000000000000000000000000000000  m80-artifacts.tar.gz\n",
    )
    .unwrap();
    let dst = dir.path().join("mismatch-dst");
    let run_root = dir.path().join("mismatch-run");

    let output = m80()
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
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("artifact tarball sha256 mismatch"),
        "quickstart must fail before extraction on external checksum mismatch; stderr={stderr}"
    );
    assert!(
        !dst.exists(),
        "quickstart must not install artifacts after checksum mismatch"
    );
}

#[test]
fn quickstart_rejects_bundled_host_binaries_manifest() {
    let dir = tempfile::tempdir().unwrap();
    let tarball = write_release_tarball_with_bundled_host_manifest(&dir);
    let dst = dir.path().join("host-manifest-dst");
    let run_root = dir.path().join("host-manifest-run");

    let output = m80()
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
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("host-binaries.manifest.json"),
        "quickstart should explain the forbidden bundled host manifest; stderr={stderr}"
    );
    assert!(
        !dst.exists(),
        "quickstart must not install artifacts after seeing a bundled host manifest"
    );
}

#[test]
fn quickstart_rejects_nested_bundled_host_binaries_manifest() {
    let dir = tempfile::tempdir().unwrap();
    let tarball = write_release_tarball_with_nested_bundled_host_manifest(&dir);
    let dst = dir.path().join("nested-host-manifest-dst");
    let run_root = dir.path().join("nested-host-manifest-run");

    let output = m80()
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
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("host-binaries.manifest.json"),
        "quickstart should reject nested bundled host manifests; stderr={stderr}"
    );
    assert!(
        !dst.exists(),
        "quickstart must not install artifacts after seeing a nested host manifest"
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
