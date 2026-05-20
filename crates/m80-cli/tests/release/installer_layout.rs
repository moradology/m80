use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

use m80_image_manifest::{
    BuildReceipt, BuildReceiptArtifactKind, InstallProvenance, InstallProvenanceArtifact,
    InstallProvenanceRewrite, Manifest,
};

#[path = "../common/mod.rs"]
mod common;
#[path = "installer_layout/finalization.rs"]
mod finalization;
#[path = "installer_layout/fixture.rs"]
mod fixture;
#[path = "installer_layout/http_fixture.rs"]
mod http_fixture;
#[path = "installer_layout/remote_fetch.rs"]
mod remote_fetch;

use common::m80;
use fixture::{
    read_repo_file, running_as_root, set_mode, sha256_hex, write_duplicate_path_bundle,
    write_release_bundle, RELEASE_TAG,
};

const REQUIRED_INSTALLED_FILES: &[&str] = &[
    "bin/m80",
    "bin/m80-jailer-harden",
    "bin/m80-net-helper",
    "artifacts/vmlinux",
    "artifacts/output.ext4",
    "artifacts/output.ext4.manifest.json",
    "artifacts/output.ext4.build-receipt.json",
    "artifacts/m80-guestd",
    "install.sh",
    "bundle.json",
    "SHA256SUMS",
];

struct HostPrereqFixture {
    _temp: tempfile::TempDir,
    firecracker_bin: PathBuf,
    firecracker_seccomp_filter: PathBuf,
    jailer_bin: PathBuf,
}

impl HostPrereqFixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let bin_dir = temp.path().join("bin");
        fs::create_dir_all(&bin_dir).unwrap();
        let firecracker_bin = bin_dir.join("firecracker");
        let jailer_bin = bin_dir.join("jailer");
        let firecracker_seccomp_filter = bin_dir.join("firecracker-seccomp-filter.bin");
        write_executable(
            &firecracker_bin,
            "#!/bin/sh\nprintf 'Firecracker v1.15.1\\n'\n",
        );
        write_executable(&jailer_bin, "#!/bin/sh\nprintf 'Jailer v1.15.1\\n'\n");
        fs::write(&firecracker_seccomp_filter, b"{\"seccomp_level\":2}\n").unwrap();

        Self {
            _temp: temp,
            firecracker_bin,
            firecracker_seccomp_filter,
            jailer_bin,
        }
    }

    fn apply(&self, cmd: &mut assert_cmd::Command) {
        clear_install_env(cmd);
        cmd.env("M80_FIRECRACKER_BIN", &self.firecracker_bin);
        cmd.env(
            "M80_FIRECRACKER_SECCOMP_FILTER",
            &self.firecracker_seccomp_filter,
        );
        cmd.env("M80_JAILER_BIN", &self.jailer_bin);
        cmd.env("M80_INSTALL_HOSTLESS_FIXTURE", "1");
    }
}

#[test]
fn install_bundle_layout_copies_verified_bundle_into_version_dir() {
    let bundle = write_release_bundle(None);
    let host = HostPrereqFixture::new();
    let install_temp = tempfile::tempdir().unwrap();
    let install_root = install_temp.path().join("install-root");

    let output = run_install(&bundle, &install_root, Some(&host), &[], &[]);

    assert!(
        output.status.success(),
        "install failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("installed bundle layout"), "{stdout}");
    assert!(stdout.contains("files_copied=12"), "{stdout}");
    assert!(stdout.contains("active_pointer_flipped=true"), "{stdout}");
    assert!(stdout.contains("profile_written=true"), "{stdout}");
    assert!(
        stdout.contains("preflight_gate=hostless_fixture"),
        "{stdout}"
    );
    assert!(
        stdout.contains("finalization_order=bundle_verification,host_prerequisite_verification,install_provenance,host_binaries_manifest,default_profile,preflight_smoke_gate,active_pointer_flip"),
        "{stdout}"
    );

    let version_dir = install_root.join("versions").join(&bundle.release_tag);
    assert!(
        stdout.contains(&format!("version_dir={}", version_dir.display())),
        "{stdout}"
    );
    for relpath in REQUIRED_INSTALLED_FILES {
        assert!(
            version_dir.join(relpath).is_file(),
            "installed bundle missing {relpath}"
        );
    }
    let provenance_path = version_dir.join("artifacts/install-provenance.json");
    assert!(
        provenance_path.is_file(),
        "installer must emit installed provenance"
    );
    assert_eq!(
        fs::read_link(install_root.join("active")).unwrap(),
        version_dir
    );

    let artifacts = version_dir.join("artifacts");
    let host_binaries_manifest = artifacts.join("host-binaries.manifest.json");
    assert!(
        host_binaries_manifest.is_file(),
        "installer must emit host-binaries manifest"
    );
    assert!(
        stdout.contains(&format!(
            "host_binaries_manifest={}",
            host_binaries_manifest.display()
        )),
        "{stdout}"
    );
    let profile_path = install_root.join("profiles/default.toml");
    assert!(
        profile_path.is_file(),
        "installer must write default profile"
    );
    assert!(
        stdout.contains(&format!("profile_path={}", profile_path.display())),
        "{stdout}"
    );
    let profile = fs::read_to_string(&profile_path).unwrap();
    assert!(profile.contains("host_binaries_manifest = "), "{profile}");
    assert!(
        profile.contains(&host_binaries_manifest.display().to_string()),
        "{profile}"
    );
    assert!(profile.contains("jailer_harden_bin = "), "{profile}");
    assert!(
        profile.contains(
            &version_dir
                .join("bin/m80-jailer-harden")
                .display()
                .to_string()
        ),
        "{profile}"
    );
    assert!(profile.contains("net_helper_bin = "), "{profile}");
    assert!(
        profile.contains(&version_dir.join("bin/m80-net-helper").display().to_string()),
        "{profile}"
    );
    let config = fs::read_to_string(install_root.join("config.toml")).unwrap();
    assert!(config.contains("default_profile = 'default'"), "{config}");

    let manifest_path = artifacts.join("output.ext4.manifest.json");
    let manifest = Manifest::read(&manifest_path).unwrap();
    assert_eq!(manifest.kernel_image, artifacts.join("vmlinux"));
    assert_eq!(manifest.output_rootfs_image, artifacts.join("output.ext4"));
    assert_eq!(manifest.daemon_binary_path, artifacts.join("m80-guestd"));
    manifest.verify(&artifacts).unwrap();

    let receipt_path = artifacts.join("output.ext4.build-receipt.json");
    let receipt = BuildReceipt::read(&receipt_path).unwrap();
    assert_eq!(receipt.manifest_path, manifest_path);
    assert_eq!(receipt.manifest_sha256, sha256_hex(&manifest_path));
    assert_receipt_artifact(
        &receipt,
        BuildReceiptArtifactKind::KernelImage,
        &artifacts.join("vmlinux"),
    );
    assert_receipt_artifact(
        &receipt,
        BuildReceiptArtifactKind::OutputRootfsImage,
        &artifacts.join("output.ext4"),
    );
    assert_receipt_artifact(
        &receipt,
        BuildReceiptArtifactKind::DaemonBinaryPath,
        &artifacts.join("m80-guestd"),
    );

    let provenance = InstallProvenance::read(&provenance_path).unwrap();
    assert_eq!(provenance.release_tag.as_deref(), Some(RELEASE_TAG));
    assert_eq!(provenance.transforms.len(), 2);
    assert_rewrite_record(
        &provenance,
        InstallProvenanceArtifact::GuestManifest,
        "artifacts/output.ext4.manifest.json",
        &manifest_path,
    );
    assert_rewrite_record(
        &provenance,
        InstallProvenanceArtifact::BuildReceipt,
        "artifacts/output.ext4.build-receipt.json",
        &receipt_path,
    );
}

#[test]
fn install_bundle_layout_missing_required_bundle_file_fails_before_activation() {
    let bundle = write_release_bundle(Some("bin/m80-net-helper"));
    let install_temp = tempfile::tempdir().unwrap();
    let install_root = install_temp.path().join("install-root");
    let previous = seed_previous_active_install(&install_root);

    let output = m80()
        .args([
            "install",
            "--bundle-url",
            &format!("file://{}", bundle.tarball.display()),
            "--install-root",
            install_root.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("required artifact missing") && stderr.contains("bin/m80-net-helper"),
        "unexpected stderr: {stderr}"
    );
    assert!(
        fs::read_link(install_root.join("active")).unwrap() == previous,
        "missing bundle file must leave previous active pointer selected"
    );
    assert!(
        !install_root.join("versions").join(RELEASE_TAG).exists(),
        "missing bundle file must not publish a version dir"
    );
}

#[test]
fn install_bundle_layout_duplicate_bundle_path_fails_before_activation() {
    let temp = tempfile::tempdir().unwrap();
    let tarball = write_duplicate_path_bundle(temp.path());
    let install_root = temp.path().join("install-root");

    let output = m80()
        .args([
            "install",
            "--bundle-url",
            &format!("file://{}", tarball.display()),
            "--install-root",
            install_root.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(6));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("bundle duplicate path: bin/m80"),
        "unexpected stderr: {stderr}"
    );
    assert!(
        !install_root.join("active").exists(),
        "duplicate bundle path must not switch active pointer"
    );
}

#[test]
fn install_bundle_layout_permission_failure_leaves_active_state_untouched() {
    if running_as_root() {
        return;
    }

    let bundle = write_release_bundle(None);
    let install_temp = tempfile::tempdir().unwrap();
    let install_root = install_temp.path().join("install-root");
    fs::create_dir(&install_root).unwrap();
    set_mode(&install_root, 0o500);

    let output = m80()
        .args([
            "install",
            "--bundle-url",
            &format!("file://{}", bundle.tarball.display()),
            "--install-root",
            install_root.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    set_mode(&install_root, 0o700);

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(
        !install_root.join("active").exists(),
        "permission failure must not switch active pointer"
    );
    assert!(
        !install_root.join("versions").join(RELEASE_TAG).exists(),
        "permission failure must not publish a version dir"
    );
}

#[test]
fn install_bundle_layout_dry_run_never_reads_or_writes_bundle_layout() {
    let bundle = write_release_bundle(None);
    let install_temp = tempfile::tempdir().unwrap();
    let install_root = install_temp.path().join("install-root");

    let output = m80()
        .args([
            "install",
            "--bundle-url",
            &format!("file://{}", bundle.tarball.display()),
            "--install-root",
            install_root.to_str().unwrap(),
            "--dry-run",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "install dry-run failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("install dry-run"), "{stdout}");
    assert!(stdout.contains("writes=none"), "{stdout}");
    assert!(
        !install_root.exists(),
        "dry-run must not create install root {}",
        install_root.display()
    );
}

fn run_install(
    bundle: &fixture::ReleaseBundleFixture,
    install_root: &Path,
    host: Option<&HostPrereqFixture>,
    envs: &[(&str, &str)],
    env_removals: &[&str],
) -> std::process::Output {
    run_install_url(
        &format!("file://{}", bundle.tarball.display()),
        install_root,
        host,
        envs,
        env_removals,
    )
}

fn run_install_url(
    bundle_url: &str,
    install_root: &Path,
    host: Option<&HostPrereqFixture>,
    envs: &[(&str, &str)],
    env_removals: &[&str],
) -> std::process::Output {
    let mut command = m80();
    command.args([
        "install",
        "--bundle-url",
        bundle_url,
        "--install-root",
        install_root.to_str().unwrap(),
    ]);
    clear_install_env(&mut command);
    for key in env_removals {
        command.env_remove(key);
    }
    if let Some(host) = host {
        host.apply(&mut command);
    }
    for (key, value) in envs {
        command.env(key, value);
    }
    command.output().unwrap()
}

fn clear_install_env(command: &mut assert_cmd::Command) {
    for key in [
        "M80_CGROUP_MODE",
        "M80_JAIL_UID",
        "M80_JAIL_GID",
        "M80_EXPECTED_CONCURRENT_VMS",
        "M80_FIRECRACKER_BIN",
        "M80_FIRECRACKER_VERSION",
        "M80_FIRECRACKER_SECCOMP_FILTER",
        "M80_JAILER_BIN",
        "M80_JAILER_HARDEN_BIN",
        "M80_NET_HELPER_BIN",
        "M80_INSTALL_HOSTLESS_FIXTURE",
        "M80_INSTALL_INJECT_INTERRUPTION_AFTER_PROFILE",
        "M80_RELEASE_ATTESTATION_GH",
    ] {
        command.env_remove(key);
    }
}

fn seed_previous_active_install(install_root: &Path) -> PathBuf {
    let previous = install_root.join("versions/v-previous");
    fs::create_dir_all(&previous).unwrap();
    fs::write(previous.join("marker"), b"previous").unwrap();
    symlink(&previous, install_root.join("active")).unwrap();
    previous
}

fn write_executable(path: &Path, body: &str) {
    fs::write(path, body).unwrap();
    set_mode(path, 0o755);
}

fn assert_receipt_artifact(
    receipt: &BuildReceipt,
    kind: BuildReceiptArtifactKind,
    installed_path: &Path,
) {
    let artifact = receipt
        .artifacts
        .iter()
        .find(|artifact| artifact.kind == kind)
        .unwrap_or_else(|| panic!("missing receipt artifact {kind:?}"));
    assert_eq!(artifact.path, installed_path);
    assert_eq!(artifact.sha256, sha256_hex(installed_path));
}

fn assert_rewrite_record(
    provenance: &InstallProvenance,
    artifact: InstallProvenanceArtifact,
    source_name: &str,
    installed_path: &Path,
) {
    let transform = provenance
        .transforms
        .iter()
        .find(|transform| transform.artifact == artifact)
        .unwrap_or_else(|| panic!("missing provenance transform for {artifact:?}"));
    assert_eq!(transform.source_path, PathBuf::from(source_name));
    assert_eq!(transform.installed_path, installed_path);
    assert_eq!(
        transform.rewrite,
        InstallProvenanceRewrite::InstallPathRewrite
    );
    assert_eq!(transform.installed_sha256, sha256_hex(installed_path));
    assert_ne!(transform.source_sha256, transform.installed_sha256);
}
