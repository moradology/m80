//! Smoke test for `m80 quickstart --no-run` with a local release-like tarball.

pub(crate) use crate::common::m80;

use std::process::Command as StdCommand;

pub(crate) use m80_image_manifest::{
    BuildReceipt, BuildReceiptArtifact, BuildReceiptArtifactKind, ImageKind, InstallProvenance,
    InstallProvenanceArtifact, InstallProvenanceRewrite, KernelKind, Manifest, RootfsFormat,
};

pub(crate) struct InstallPaths {
    pub(crate) dst: std::path::PathBuf,
    pub(crate) run_root: std::path::PathBuf,
    pub(crate) profile_dir: std::path::PathBuf,
    pub(crate) config_path: std::path::PathBuf,
}

pub(crate) fn install_paths(dir: &tempfile::TempDir, name: &str) -> InstallPaths {
    InstallPaths {
        dst: dir.path().join(format!("{name}-dst")),
        run_root: dir.path().join(format!("{name}-run")),
        profile_dir: dir.path().join(format!("{name}-profiles")),
        config_path: dir.path().join(format!("{name}-config.toml")),
    }
}

pub(crate) fn quickstart_no_run_args(
    tarball: &std::path::Path,
    paths: &InstallPaths,
) -> Vec<String> {
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

pub(crate) fn read_toml(path: &std::path::Path) -> toml::Value {
    std::fs::read_to_string(path)
        .unwrap()
        .parse::<toml::Value>()
        .unwrap()
}

pub(crate) fn toml_str<'a>(value: &'a toml::Value, key: &str) -> &'a str {
    value
        .get(key)
        .and_then(toml::Value::as_str)
        .unwrap_or_else(|| panic!("missing string field {key} in {value:?}"))
}

pub(crate) fn run_checked(cmd: &mut StdCommand, label: &str) {
    let output = cmd.output().unwrap();
    assert!(
        output.status.success(),
        "{label} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

pub(crate) fn output_checked(cmd: &mut StdCommand, label: &str) -> std::process::Output {
    let output = cmd.output().unwrap();
    assert!(
        output.status.success(),
        "{label} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

pub(crate) fn write_release_tarball(dir: &tempfile::TempDir) -> std::path::PathBuf {
    write_release_tarball_with_kernel_kind(dir, KernelKind::Stock)
}

pub(crate) fn write_release_tarball_with_kernel_kind(
    dir: &tempfile::TempDir,
    kernel_kind: KernelKind,
) -> std::path::PathBuf {
    write_release_tarball_inner(dir, None, kernel_kind)
}

pub(crate) fn write_release_tarball_with_bundled_host_manifest(
    dir: &tempfile::TempDir,
) -> std::path::PathBuf {
    write_release_tarball_inner(dir, Some("host-binaries.manifest.json"), KernelKind::Stock)
}

pub(crate) fn write_release_tarball_with_nested_bundled_host_manifest(
    dir: &tempfile::TempDir,
) -> std::path::PathBuf {
    write_release_tarball_inner(
        dir,
        Some("nested/host-binaries.manifest.json"),
        KernelKind::Stock,
    )
}

pub(crate) fn write_release_tarball_with_extra_file(
    dir: &tempfile::TempDir,
    relpath: &str,
) -> std::path::PathBuf {
    write_release_tarball_inner(dir, Some(relpath), KernelKind::Stock)
}

pub(crate) fn write_release_tarball_inner(
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
  "conditional_binaries": [],
  "launch_material": [
    {
      "name": "firecracker_seccomp_filter",
      "path": "/opt/firecracker/bin/firecracker-seccomp-filter.bin",
      "sha256": "0000000000000000000000000000000000000000000000000000000000000000",
      "version": "v1.15.1"
    }
  ],
  "schema_version": 5
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

pub(crate) fn write_manifest_with_stale_paths(src: &std::path::Path, kernel_kind: KernelKind) {
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

pub(crate) fn sha256_hex(path: &std::path::Path) -> String {
    let output = output_checked(StdCommand::new("sha256sum").arg(path), "sha256sum");
    String::from_utf8(output.stdout)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .to_owned()
}

pub(crate) fn assert_rewrite_record(
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

pub(crate) fn write_build_receipt_with_stale_paths(src: &std::path::Path) {
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
