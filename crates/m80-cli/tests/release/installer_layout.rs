use std::fs;
use std::path::{Path, PathBuf};

use m80_image_manifest::{
    BuildReceipt, BuildReceiptArtifactKind, InstallProvenance, InstallProvenanceArtifact,
    InstallProvenanceRewrite, Manifest,
};

#[path = "../common/mod.rs"]
mod common;
#[path = "installer_layout/fixture.rs"]
mod fixture;

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

#[test]
fn install_bundle_layout_copies_verified_bundle_into_version_dir() {
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
        ])
        .output()
        .unwrap();

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
    assert!(stdout.contains("active_pointer_unchanged=true"), "{stdout}");
    assert!(stdout.contains("profile_written=false"), "{stdout}");

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
    assert!(
        !install_root.join("active").exists(),
        "layout leaf must not switch active pointer"
    );

    let artifacts = version_dir.join("artifacts");
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
        !install_root.join("active").exists(),
        "missing bundle file must not switch active pointer"
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

#[test]
fn installed_layout_doc_names_directory_contract_and_tests() {
    let doc = read_repo_file("docs/behaviors/release/installed-layout.md");

    for required in [
        "`<install-root>/versions/<release_tag>`",
        "`bin/m80`",
        "`artifacts/output.ext4.manifest.json`",
        "`artifacts/install-provenance.json`",
        "`<install-root>/active`",
        "install_bundle_layout_copies_verified_bundle_into_version_dir",
        "install_bundle_layout_missing_required_bundle_file_fails_before_activation",
        "install_bundle_layout_duplicate_bundle_path_fails_before_activation",
        "install_bundle_layout_permission_failure_leaves_active_state_untouched",
        "install_bundle_layout_dry_run_never_reads_or_writes_bundle_layout",
    ] {
        assert!(
            doc.contains(required),
            "installed layout doc missing {required:?}"
        );
    }
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
