use std::path::{Path, PathBuf};
use std::process::Command;

use m80_firecracker::FcError;
use m80_image_manifest::{
    BuildReceipt, BuildReceiptArtifact, BuildReceiptArtifactKind, InstallProvenance,
    InstallProvenanceArtifact, InstallProvenanceRewrite, InstallProvenanceTransform,
};

use super::process::run_output_capture;

const INSTALL_PROVENANCE_FILE: &str = "install-provenance.json";

pub(super) fn relocate_manifest(
    artifact_dir: &Path,
) -> Result<InstallProvenanceTransform, FcError> {
    let manifest_path = artifact_dir.join("output.ext4.manifest.json");
    let source_sha256 = sha256_file(&manifest_path)?;
    let mut manifest = m80_image_manifest::Manifest::read(&manifest_path)?;
    manifest.kernel_image = artifact_dir.join("vmlinux");
    manifest.output_rootfs_image = artifact_dir.join("output.ext4");
    manifest.daemon_binary_path = artifact_dir.join("m80-guestd");
    if manifest.source_rootfs_image.is_some() {
        manifest.source_rootfs_image = Some(artifact_dir.join("source.ext4"));
    }
    manifest.write(&manifest_path).map_err(FcError::Manifest)?;
    manifest.verify(artifact_dir).map_err(FcError::Manifest)?;
    let installed_sha256 = sha256_file(&manifest_path)?;
    Ok(install_path_rewrite_transform(
        InstallProvenanceArtifact::GuestManifest,
        "output.ext4.manifest.json",
        manifest_path,
        source_sha256,
        installed_sha256,
    ))
}

pub(super) fn relocate_build_receipt(
    artifact_dir: &Path,
) -> Result<InstallProvenanceTransform, FcError> {
    let manifest_path = artifact_dir.join("output.ext4.manifest.json");
    let receipt_path = artifact_dir.join("output.ext4.build-receipt.json");
    let source_sha256 = sha256_file(&receipt_path)?;
    let manifest = m80_image_manifest::Manifest::read(&manifest_path)?;
    let manifest_sha256 = sha256_file(&manifest_path)?;
    let mut artifacts = vec![
        receipt_artifact(
            BuildReceiptArtifactKind::KernelImage,
            manifest.kernel_image.clone(),
            manifest.kernel_image_sha256.clone(),
        ),
        receipt_artifact(
            BuildReceiptArtifactKind::OutputRootfsImage,
            manifest.output_rootfs_image.clone(),
            manifest.output_rootfs_sha256.clone(),
        ),
        receipt_artifact(
            BuildReceiptArtifactKind::DaemonBinaryPath,
            manifest.daemon_binary_path.clone(),
            manifest.daemon_binary_sha256.clone(),
        ),
    ];
    if let (Some(path), Some(sha256)) = (
        manifest.source_rootfs_image.clone(),
        manifest.source_rootfs_sha256.clone(),
    ) {
        artifacts.push(receipt_artifact(
            BuildReceiptArtifactKind::SourceRootfsImage,
            path,
            sha256,
        ));
    }
    BuildReceipt::new(manifest_path, manifest_sha256, artifacts)
        .write(&receipt_path)
        .map_err(FcError::Manifest)?;
    let installed_sha256 = sha256_file(&receipt_path)?;
    Ok(install_path_rewrite_transform(
        InstallProvenanceArtifact::BuildReceipt,
        "output.ext4.build-receipt.json",
        receipt_path,
        source_sha256,
        installed_sha256,
    ))
}

fn receipt_artifact(
    kind: BuildReceiptArtifactKind,
    path: PathBuf,
    sha256: String,
) -> BuildReceiptArtifact {
    BuildReceiptArtifact { kind, path, sha256 }
}

fn install_path_rewrite_transform(
    artifact: InstallProvenanceArtifact,
    source_name: &str,
    installed_path: PathBuf,
    source_sha256: String,
    installed_sha256: String,
) -> InstallProvenanceTransform {
    InstallProvenanceTransform {
        artifact,
        source_sha256,
        source_path: PathBuf::from(source_name),
        installed_sha256,
        installed_path,
        rewrite: InstallProvenanceRewrite::InstallPathRewrite,
    }
}

pub(super) fn write_install_provenance(
    artifact_url: &str,
    artifact_dir: &Path,
    transforms: Vec<InstallProvenanceTransform>,
) -> Result<(), FcError> {
    InstallProvenance::new(release_tag_from_artifact_url(artifact_url), transforms)
        .write(&artifact_dir.join(INSTALL_PROVENANCE_FILE))
        .map_err(FcError::Manifest)
}

pub(super) fn release_tag_from_artifact_url(artifact_url: &str) -> Option<String> {
    let (_, tail) = artifact_url.split_once("/releases/download/")?;
    let tag = tail.split('/').next()?;
    if tag.is_empty() || tag == "latest" {
        None
    } else {
        Some(tag.to_owned())
    }
}

fn sha256_file(path: &Path) -> Result<String, FcError> {
    let output = run_output_capture(Command::new("sha256sum").arg(path), "compute file checksum")?;
    if !output.status.success() {
        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let output_text = if combined.is_empty() {
            String::new()
        } else {
            format!(": {combined}")
        };
        return Err(FcError::CommandFailed {
            command: "compute file checksum",
            status: output.status,
            output: output_text,
        });
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let digest = stdout.split_whitespace().next().ok_or_else(|| {
        FcError::Config(m80_firecracker::ConfigError::InvalidValue {
            field: "sha256sum",
            reason: "sha256sum produced no digest".to_owned(),
        })
    })?;
    Ok(digest.to_owned())
}
