//! Smoke test for `m80 quickstart --no-run` with a local release-like tarball.

mod common;

use common::m80;

use std::process::Command as StdCommand;

use m80_image_manifest::{
    BuildReceipt, BuildReceiptArtifact, BuildReceiptArtifactKind, ImageKind, InstallProvenance,
    InstallProvenanceArtifact, InstallProvenanceRewrite, KernelKind, Manifest, RootfsFormat,
};
use serde_json::Value;

struct InstallPaths {
    dst: std::path::PathBuf,
    run_root: std::path::PathBuf,
    profile_dir: std::path::PathBuf,
    config_path: std::path::PathBuf,
}

fn install_paths(dir: &tempfile::TempDir, name: &str) -> InstallPaths {
    InstallPaths {
        dst: dir.path().join(format!("{name}-dst")),
        run_root: dir.path().join(format!("{name}-run")),
        profile_dir: dir.path().join(format!("{name}-profiles")),
        config_path: dir.path().join(format!("{name}-config.toml")),
    }
}

fn quickstart_no_run_args(tarball: &std::path::Path, paths: &InstallPaths) -> Vec<String> {
    vec![
        "quickstart".to_owned(),
        "--artifact-url".to_owned(),
        format!("file://{}", tarball.display()),
        "--artifact-dir".to_owned(),
        paths.dst.display().to_string(),
        "--run-root".to_owned(),
        paths.run_root.display().to_string(),
        "--profile-dir".to_owned(),
        paths.profile_dir.display().to_string(),
        "--config-path".to_owned(),
        paths.config_path.display().to_string(),
        "--no-run".to_owned(),
    ]
}

fn read_toml(path: &std::path::Path) -> toml::Value {
    std::fs::read_to_string(path)
        .unwrap()
        .parse::<toml::Value>()
        .unwrap()
}

fn toml_str<'a>(value: &'a toml::Value, key: &str) -> &'a str {
    value
        .get(key)
        .and_then(toml::Value::as_str)
        .unwrap_or_else(|| panic!("missing string field {key} in {value:?}"))
}

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
    write_release_tarball_with_kernel_kind(dir, KernelKind::Stock)
}

fn write_release_tarball_with_kernel_kind(
    dir: &tempfile::TempDir,
    kernel_kind: KernelKind,
) -> std::path::PathBuf {
    write_release_tarball_inner(dir, None, kernel_kind)
}

fn write_release_tarball_with_bundled_host_manifest(dir: &tempfile::TempDir) -> std::path::PathBuf {
    write_release_tarball_inner(dir, Some("host-binaries.manifest.json"), KernelKind::Stock)
}

fn write_release_tarball_with_nested_bundled_host_manifest(
    dir: &tempfile::TempDir,
) -> std::path::PathBuf {
    write_release_tarball_inner(
        dir,
        Some("nested/host-binaries.manifest.json"),
        KernelKind::Stock,
    )
}

fn write_release_tarball_with_extra_file(
    dir: &tempfile::TempDir,
    relpath: &str,
) -> std::path::PathBuf {
    write_release_tarball_inner(dir, Some(relpath), KernelKind::Stock)
}

fn write_release_tarball_inner(
    dir: &tempfile::TempDir,
    extra_relpath: Option<&str>,
    kernel_kind: KernelKind,
) -> std::path::PathBuf {
    let src = dir.path().join("src");
    std::fs::create_dir(&src).unwrap();
    std::fs::write(src.join("vmlinux"), b"kernel").unwrap();
    std::fs::write(src.join("output.ext4"), b"rootfs").unwrap();
    std::fs::write(src.join("m80-guestd"), b"guestd").unwrap();
    write_manifest_with_stale_paths(&src, kernel_kind);
    write_build_receipt_with_stale_paths(&src);
    let mut checksum_inputs = vec![
        "vmlinux",
        "output.ext4",
        "output.ext4.manifest.json",
        "output.ext4.build-receipt.json",
        "m80-guestd",
    ];
    if let Some(relpath) = extra_relpath {
        let extra_file = src.join(relpath);
        std::fs::create_dir_all(extra_file.parent().unwrap()).unwrap();
        let extra_payload: &[u8] = if relpath.ends_with("host-binaries.manifest.json") {
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
"#
        } else {
            b"operator-provided host prerequisite payload\n"
        };
        std::fs::write(&extra_file, extra_payload).unwrap();
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

fn write_manifest_with_stale_paths(src: &std::path::Path, kernel_kind: KernelKind) {
    let stale = std::path::PathBuf::from("/tmp/m80-release-artifacts");
    let manifest = Manifest::new(
        stale.join("m80-guestd"),
        sha256_hex(&src.join("m80-guestd")),
        "v1.15.1".to_owned(),
        m80_proto::GUEST_PORT_DEFAULT,
        ImageKind::Minimal,
        stale.join("vmlinux"),
        sha256_hex(&src.join("vmlinux")),
        kernel_kind,
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
    let paths = install_paths(&dir, "default");

    m80()
        .args(quickstart_no_run_args(&tarball, &paths))
        .assert()
        .success();

    for artifact in [
        "vmlinux",
        "output.ext4",
        "output.ext4.manifest.json",
        "m80-guestd",
    ] {
        assert!(
            paths.dst.join(artifact).is_file(),
            "quickstart should install {artifact}"
        );
    }
    assert!(paths.run_root.is_dir(), "quickstart should create run-root");

    let manifest = Manifest::read(&paths.dst.join("output.ext4.manifest.json")).unwrap();
    assert_eq!(manifest.kernel_image, paths.dst.join("vmlinux"));
    assert_eq!(manifest.output_rootfs_image, paths.dst.join("output.ext4"));
    assert_eq!(manifest.daemon_binary_path, paths.dst.join("m80-guestd"));
    manifest.verify(&paths.dst).unwrap();
    let receipt = BuildReceipt::read(&paths.dst.join("output.ext4.build-receipt.json")).unwrap();
    assert_eq!(
        receipt.manifest_path,
        paths.dst.join("output.ext4.manifest.json")
    );
    assert_eq!(
        receipt.manifest_sha256,
        sha256_hex(&paths.dst.join("output.ext4.manifest.json"))
    );
    let provenance = InstallProvenance::read(&paths.dst.join("install-provenance.json")).unwrap();
    assert_eq!(provenance.release_tag, None);
    assert_eq!(provenance.transforms.len(), 2);
    assert_rewrite_record(
        &provenance,
        InstallProvenanceArtifact::GuestManifest,
        "output.ext4.manifest.json",
        &paths.dst.join("output.ext4.manifest.json"),
    );
    assert_rewrite_record(
        &provenance,
        InstallProvenanceArtifact::BuildReceipt,
        "output.ext4.build-receipt.json",
        &paths.dst.join("output.ext4.build-receipt.json"),
    );
    assert!(
        !paths.dst.join("host-binaries.manifest.json").exists(),
        "quickstart must not install a bundled host-binaries manifest"
    );

    let profile_path = paths.profile_dir.join("default.toml");
    let profile = read_toml(&profile_path);
    assert_eq!(
        toml_str(&profile, "artifact_dir"),
        paths.dst.to_str().unwrap()
    );
    assert_eq!(
        toml_str(&profile, "kernel_image"),
        paths.dst.join("vmlinux").to_str().unwrap()
    );
    assert_eq!(
        toml_str(&profile, "rootfs_image"),
        paths.dst.join("output.ext4").to_str().unwrap()
    );
    assert_eq!(toml_str(&profile, "kernel_kind"), "stock");
    assert_eq!(
        toml_str(&profile, "guestd"),
        paths.dst.join("m80-guestd").to_str().unwrap()
    );
    assert_eq!(
        toml_str(&profile, "guest_manifest"),
        paths
            .dst
            .join("output.ext4.manifest.json")
            .to_str()
            .unwrap()
    );
    assert_eq!(
        toml_str(&profile, "build_receipt"),
        paths
            .dst
            .join("output.ext4.build-receipt.json")
            .to_str()
            .unwrap()
    );
    assert_eq!(
        toml_str(&profile, "install_provenance"),
        paths.dst.join("install-provenance.json").to_str().unwrap()
    );
    assert_eq!(
        toml_str(&profile, "host_binaries_manifest"),
        paths
            .dst
            .join("host-binaries.manifest.json")
            .to_str()
            .unwrap()
    );
    assert_eq!(
        toml_str(&profile, "run_root"),
        paths.run_root.to_str().unwrap()
    );
    assert_eq!(
        toml_str(&profile, "m80_version"),
        concat!(env!("CARGO_PKG_VERSION"), "-dev")
    );
    assert!(profile.get("release_tag").is_none());

    let config = read_toml(&paths.config_path);
    assert_eq!(toml_str(&config, "default_profile"), "default");
    assert_eq!(
        toml_str(&config, "run_root"),
        paths.run_root.to_str().unwrap()
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
fn quickstart_profile_records_release_tag_from_download_url() {
    let dir = tempfile::tempdir().unwrap();
    let source_tarball = write_release_tarball(&dir);
    let release_dir = dir.path().join("github/releases/download/v9.8.7");
    std::fs::create_dir_all(&release_dir).unwrap();
    let tarball = release_dir.join("m80-linux-x86_64-minimal-artifacts.tar.gz");
    std::fs::copy(&source_tarball, &tarball).unwrap();
    let tarball_sha = sha256_hex(&tarball);
    std::fs::write(
        format!("{}.sha256", tarball.display()),
        format!(
            "{tarball_sha}  {}\n",
            tarball.file_name().unwrap().to_string_lossy()
        ),
    )
    .unwrap();
    let paths = install_paths(&dir, "tagged");

    m80()
        .args(quickstart_no_run_args(&tarball, &paths))
        .assert()
        .success();

    let profile = read_toml(&paths.profile_dir.join("default.toml"));
    assert_eq!(toml_str(&profile, "release_tag"), "v9.8.7");
}

#[test]
fn quickstart_profile_uses_installed_manifest_kernel_kind() {
    let dir = tempfile::tempdir().unwrap();
    let tarball = write_release_tarball_with_kernel_kind(&dir, KernelKind::Stripped);
    let paths = install_paths(&dir, "stripped");

    m80()
        .args(quickstart_no_run_args(&tarball, &paths))
        .assert()
        .success();

    let profile = read_toml(&paths.profile_dir.join("default.toml"));
    assert_eq!(toml_str(&profile, "kernel_kind"), "stripped");
}

#[test]
fn quickstart_overwrites_stale_profile_and_config() {
    let dir = tempfile::tempdir().unwrap();
    let tarball = write_release_tarball(&dir);
    let paths = install_paths(&dir, "stale");
    std::fs::create_dir_all(&paths.profile_dir).unwrap();
    std::fs::write(paths.profile_dir.join("default.toml"), "stale = true\n").unwrap();
    std::fs::write(
        &paths.config_path,
        "default_profile = \"old\"\nrun_root = \"/old\"\n",
    )
    .unwrap();

    m80()
        .args(quickstart_no_run_args(&tarball, &paths))
        .assert()
        .success();

    let profile_text = std::fs::read_to_string(paths.profile_dir.join("default.toml")).unwrap();
    assert!(!profile_text.contains("stale = true"));
    let profile = profile_text.parse::<toml::Value>().unwrap();
    assert_eq!(
        toml_str(&profile, "artifact_dir"),
        paths.dst.to_str().unwrap()
    );
    let config = read_toml(&paths.config_path);
    assert_eq!(toml_str(&config, "default_profile"), "default");
    assert_eq!(
        toml_str(&config, "run_root"),
        paths.run_root.to_str().unwrap()
    );
}

#[test]
fn quickstart_rolls_back_previous_profile_when_config_write_fails() {
    let dir = tempfile::tempdir().unwrap();
    let tarball = write_release_tarball(&dir);
    let mut paths = install_paths(&dir, "rollback");
    std::fs::create_dir_all(&paths.profile_dir).unwrap();
    let profile_path = paths.profile_dir.join("default.toml");
    let stale_profile = "kernel_image = \"/old/vmlinux\"\nrootfs_image = \"/old/rootfs.ext4\"\n";
    std::fs::write(&profile_path, stale_profile).unwrap();
    paths.config_path = dir.path().join("rollback-config-dir");
    std::fs::create_dir(&paths.config_path).unwrap();

    let output = m80()
        .args(quickstart_no_run_args(&tarball, &paths))
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert_eq!(
        std::fs::read_to_string(profile_path).unwrap(),
        stale_profile
    );
}

#[test]
fn quickstart_rolls_back_symlinked_profile_target_when_config_write_fails() {
    let dir = tempfile::tempdir().unwrap();
    let tarball = write_release_tarball(&dir);
    let mut paths = install_paths(&dir, "symlink-rollback");
    std::fs::create_dir_all(&paths.profile_dir).unwrap();
    let profile_path = paths.profile_dir.join("default.toml");
    let profile_target = paths.profile_dir.join("default-target.toml");
    let stale_profile = "kernel_image = \"/old/vmlinux\"\nrootfs_image = \"/old/rootfs.ext4\"\n";
    std::fs::write(&profile_target, stale_profile).unwrap();
    std::os::unix::fs::symlink("default-target.toml", &profile_path).unwrap();
    paths.config_path = dir.path().join("symlink-rollback-config-dir");
    std::fs::create_dir(&paths.config_path).unwrap();

    let output = m80()
        .args(quickstart_no_run_args(&tarball, &paths))
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert_eq!(
        std::fs::read_to_string(profile_target).unwrap(),
        stale_profile
    );
    assert_eq!(
        std::fs::read_link(profile_path).unwrap(),
        std::path::PathBuf::from("default-target.toml")
    );
}

#[test]
fn quickstart_rejects_unknown_existing_config_key_before_profile_commit() {
    let dir = tempfile::tempdir().unwrap();
    let tarball = write_release_tarball(&dir);
    let paths = install_paths(&dir, "bad-config");
    std::fs::create_dir_all(&paths.profile_dir).unwrap();
    let profile_path = paths.profile_dir.join("default.toml");
    let stale_profile = "kernel_image = \"/old/vmlinux\"\nrootfs_image = \"/old/rootfs.ext4\"\n";
    std::fs::write(&profile_path, stale_profile).unwrap();
    std::fs::write(&paths.config_path, "unknown_key = true\n").unwrap();

    let output = m80()
        .args(quickstart_no_run_args(&tarball, &paths))
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unknown config key"),
        "quickstart should report the config key that would make the next run fail; stderr={stderr}"
    );
    assert_eq!(
        std::fs::read_to_string(profile_path).unwrap(),
        stale_profile
    );
}

#[test]
fn quickstart_json_no_run_keeps_stdout_machine_readable() {
    let dir = tempfile::tempdir().unwrap();
    let tarball = write_release_tarball(&dir);
    let paths = install_paths(&dir, "json");

    let output = m80()
        .arg("--json")
        .args(quickstart_no_run_args(&tarball, &paths))
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
        Some(paths.dst.to_str().unwrap())
    );
    assert_eq!(
        value["data"]["profile_path"].as_str(),
        Some(paths.profile_dir.join("default.toml").to_str().unwrap())
    );
    assert_eq!(
        value["data"]["config_path"].as_str(),
        Some(paths.config_path.to_str().unwrap())
    );
    assert_eq!(
        value["data"]["probe_command"].as_str(),
        Some("m80 run -- echo hello")
    );
    assert_eq!(
        value["data"]["probe_egress_policy"].as_str(),
        Some("default-outbound")
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
fn quickstart_checks_host_substrate_before_active_install_paths() {
    let dir = tempfile::tempdir().unwrap();
    let tarball = write_release_tarball(&dir);
    let dst = dir.path().join("substrate-dst");
    let run_root = dir.path().join("substrate-run");

    let output = m80()
        .env("M80_CGROUP_MODE", "bogus")
        .args([
            "quickstart",
            "--artifact-url",
            &format!("file://{}", tarball.display()),
            "--artifact-dir",
            dst.to_str().unwrap(),
            "--run-root",
            run_root.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("invalid cgroup mode"),
        "quickstart should report the typed substrate config failure; stderr={stderr}"
    );
    assert!(
        !dst.exists(),
        "quickstart must not create active artifact dir before substrate passes"
    );
    assert!(
        !run_root.exists(),
        "quickstart must not create run-root before substrate passes"
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
fn quickstart_rejects_bundled_operator_host_prerequisites() {
    for relpath in [
        "firecracker",
        "nested/jailer",
        "nested/firecracker-seccomp-filter.bin",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let tarball = write_release_tarball_with_extra_file(&dir, relpath);
        let dst = dir.path().join("host-prereq-dst");
        let run_root = dir.path().join("host-prereq-run");

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

        assert!(
            !output.status.success(),
            "quickstart should reject bundled host prerequisite {relpath}"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains(
                std::path::Path::new(relpath)
                    .file_name()
                    .unwrap()
                    .to_str()
                    .unwrap()
            ),
            "quickstart should name forbidden host prerequisite {relpath}; stderr={stderr}"
        );
        assert!(
            stderr.contains("operator-provided host prerequisites"),
            "quickstart should explain the host-prereq ownership policy; stderr={stderr}"
        );
        assert!(
            !dst.exists(),
            "quickstart must not install artifacts after seeing bundled host prerequisite {relpath}"
        );
    }
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
