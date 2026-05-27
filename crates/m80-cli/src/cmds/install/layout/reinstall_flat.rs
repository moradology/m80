use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use m80_firecracker::FcError;
use m80_image_manifest::{
    BuildReceipt, BuildReceiptArtifactKind, InstallProvenance, InstallProvenanceArtifact,
    InstallProvenanceRewrite, Manifest,
};

use super::bundle;
use super::reinstall::reinstall_error;

pub(super) fn verify_flat_projection(
    install_root: &Path,
    final_dir: &Path,
    release_tag: &str,
    repair_command: &str,
) -> Result<(), FcError> {
    let flat_bin = install_root.join("bin");
    let flat_artifacts = install_root.join("artifacts");
    for name in ["m80", "m80-jailer-harden", "m80-net-helper"] {
        require_same_inode(
            &final_dir.join("bin").join(name),
            &flat_bin.join(name),
            repair_command,
        )?;
    }
    for name in ["m80-guestd", "output.ext4", "vmlinux"] {
        require_same_inode(
            &final_dir.join("artifacts").join(name),
            &flat_artifacts.join(name),
            repair_command,
        )?;
    }

    let manifest_path = flat_artifacts.join("output.ext4.manifest.json");
    let manifest = Manifest::read(&manifest_path).map_err(|err| {
        reinstall_error(
            format!("installed flat guest manifest is unreadable: {err}"),
            repair_command,
        )
    })?;
    require_manifest_path(
        "flat guest manifest kernel_image",
        &manifest.kernel_image,
        &flat_artifacts.join("vmlinux"),
        repair_command,
    )?;
    require_manifest_path(
        "flat guest manifest output_rootfs_image",
        &manifest.output_rootfs_image,
        &flat_artifacts.join("output.ext4"),
        repair_command,
    )?;
    require_manifest_path(
        "flat guest manifest daemon_binary_path",
        &manifest.daemon_binary_path,
        &flat_artifacts.join("m80-guestd"),
        repair_command,
    )?;
    manifest.verify(&flat_artifacts).map_err(|err| {
        reinstall_error(
            format!("installed flat guest manifest is stale: {err}"),
            repair_command,
        )
    })?;

    let receipt_path = flat_artifacts.join("output.ext4.build-receipt.json");
    let receipt = BuildReceipt::read(&receipt_path).map_err(|err| {
        reinstall_error(
            format!("installed flat build receipt is unreadable: {err}"),
            repair_command,
        )
    })?;
    require_manifest_path(
        "flat build receipt manifest_path",
        &receipt.manifest_path,
        &manifest_path,
        repair_command,
    )?;
    let manifest_sha256 = bundle::sha256_file(&manifest_path).map_err(|err| {
        reinstall_error(
            format!("installed flat guest manifest could not be hashed: {err}"),
            repair_command,
        )
    })?;
    if receipt.manifest_sha256 != manifest_sha256 {
        return Err(reinstall_error(
            format!(
                "installed flat build receipt manifest_sha256 mismatch: expected={} observed={}",
                manifest_sha256, receipt.manifest_sha256
            ),
            repair_command,
        ));
    }
    require_receipt_artifact(
        &receipt,
        BuildReceiptArtifactKind::KernelImage,
        &flat_artifacts.join("vmlinux"),
        repair_command,
    )?;
    require_receipt_artifact(
        &receipt,
        BuildReceiptArtifactKind::OutputRootfsImage,
        &flat_artifacts.join("output.ext4"),
        repair_command,
    )?;
    require_receipt_artifact(
        &receipt,
        BuildReceiptArtifactKind::DaemonBinaryPath,
        &flat_artifacts.join("m80-guestd"),
        repair_command,
    )?;

    let provenance_path = flat_artifacts.join("install-provenance.json");
    let provenance = InstallProvenance::read(&provenance_path).map_err(|err| {
        reinstall_error(
            format!("installed flat install provenance is unreadable: {err}"),
            repair_command,
        )
    })?;
    if provenance.release_tag.as_deref() != Some(release_tag) {
        return Err(reinstall_error(
            format!(
                "installed flat install provenance release_tag mismatch: expected={} observed={:?}",
                release_tag, provenance.release_tag
            ),
            repair_command,
        ));
    }
    require_provenance_transform(
        &provenance,
        InstallProvenanceArtifact::GuestManifest,
        "artifacts/output.ext4.manifest.json",
        &final_dir.join("artifacts/output.ext4.manifest.json"),
        &manifest_path,
        repair_command,
    )?;
    require_provenance_transform(
        &provenance,
        InstallProvenanceArtifact::BuildReceipt,
        "artifacts/output.ext4.build-receipt.json",
        &final_dir.join("artifacts/output.ext4.build-receipt.json"),
        &receipt_path,
        repair_command,
    )
}

fn require_same_inode(
    expected: &Path,
    observed: &Path,
    repair_command: &str,
) -> Result<(), FcError> {
    let expected_metadata = fs::symlink_metadata(expected).map_err(|source| {
        reinstall_error(
            format!(
                "installed versioned projection source is unreadable: path={} source={source}",
                expected.display()
            ),
            repair_command,
        )
    })?;
    let observed_metadata = fs::symlink_metadata(observed).map_err(|source| {
        reinstall_error(
            format!(
                "installed flat projection target is unreadable: path={} source={source}",
                observed.display()
            ),
            repair_command,
        )
    })?;
    if expected_metadata.file_type().is_symlink()
        || observed_metadata.file_type().is_symlink()
        || !expected_metadata.is_file()
        || !observed_metadata.is_file()
    {
        return Err(reinstall_error(
            format!(
                "installed flat projection must be regular hardlinked files: expected={} observed={}",
                expected.display(),
                observed.display()
            ),
            repair_command,
        ));
    }
    if expected_metadata.dev() == observed_metadata.dev()
        && expected_metadata.ino() == observed_metadata.ino()
    {
        return Ok(());
    }
    Err(reinstall_error(
        format!(
            "installed flat projection hardlink mismatch: expected={} observed={}",
            expected.display(),
            observed.display()
        ),
        repair_command,
    ))
}

fn require_manifest_path(
    field: &'static str,
    observed: &Path,
    expected: &Path,
    repair_command: &str,
) -> Result<(), FcError> {
    if observed == expected {
        Ok(())
    } else {
        Err(reinstall_error(
            format!(
                "installed {field} mismatch: expected={} observed={}",
                expected.display(),
                observed.display()
            ),
            repair_command,
        ))
    }
}

fn require_receipt_artifact(
    receipt: &BuildReceipt,
    kind: BuildReceiptArtifactKind,
    expected_path: &Path,
    repair_command: &str,
) -> Result<(), FcError> {
    let mut matches = receipt
        .artifacts
        .iter()
        .filter(|artifact| artifact.kind == kind);
    let Some(artifact) = matches.next() else {
        return Err(reinstall_error(
            format!("installed flat build receipt missing artifact {kind:?}"),
            repair_command,
        ));
    };
    if matches.next().is_some() {
        return Err(reinstall_error(
            format!("installed flat build receipt duplicates artifact {kind:?}"),
            repair_command,
        ));
    }
    require_manifest_path(
        "flat build receipt artifact path",
        &artifact.path,
        expected_path,
        repair_command,
    )?;
    let expected_sha256 = bundle::sha256_file(expected_path).map_err(|err| {
        reinstall_error(
            format!(
                "installed flat build receipt artifact could not be hashed: path={} source={err}",
                expected_path.display()
            ),
            repair_command,
        )
    })?;
    if artifact.sha256 == expected_sha256 {
        Ok(())
    } else {
        Err(reinstall_error(
            format!(
                "installed flat build receipt artifact sha256 mismatch for {kind:?}: expected={} observed={}",
                expected_sha256, artifact.sha256
            ),
            repair_command,
        ))
    }
}

fn require_provenance_transform(
    provenance: &InstallProvenance,
    artifact: InstallProvenanceArtifact,
    expected_source_path: &str,
    expected_source_file: &Path,
    expected_installed_file: &Path,
    repair_command: &str,
) -> Result<(), FcError> {
    let mut matches = provenance
        .transforms
        .iter()
        .filter(|transform| transform.artifact == artifact);
    let Some(transform) = matches.next() else {
        return Err(reinstall_error(
            format!("installed flat install provenance missing transform {artifact:?}"),
            repair_command,
        ));
    };
    if matches.next().is_some() {
        return Err(reinstall_error(
            format!("installed flat install provenance duplicates transform {artifact:?}"),
            repair_command,
        ));
    }
    if transform.rewrite != InstallProvenanceRewrite::InstallPathRewrite {
        return Err(reinstall_error(
            format!(
                "installed flat install provenance rewrite mismatch for {artifact:?}: observed={:?}",
                transform.rewrite
            ),
            repair_command,
        ));
    }
    if transform.source_path != Path::new(expected_source_path) {
        return Err(reinstall_error(
            format!(
                "installed flat install provenance source_path mismatch for {artifact:?}: expected={} observed={}",
                expected_source_path,
                transform.source_path.display()
            ),
            repair_command,
        ));
    }
    require_manifest_path(
        "flat install provenance installed_path",
        &transform.installed_path,
        expected_installed_file,
        repair_command,
    )?;
    let expected_source_sha256 = bundle::sha256_file(expected_source_file).map_err(|err| {
        reinstall_error(
            format!(
                "installed flat install provenance source could not be hashed: path={} source={err}",
                expected_source_file.display()
            ),
            repair_command,
        )
    })?;
    if transform.source_sha256 != expected_source_sha256 {
        return Err(reinstall_error(
            format!(
                "installed flat install provenance source_sha256 mismatch for {artifact:?}: expected={} observed={}",
                expected_source_sha256, transform.source_sha256
            ),
            repair_command,
        ));
    }
    let expected_installed_sha256 = bundle::sha256_file(expected_installed_file).map_err(|err| {
        reinstall_error(
            format!(
                "installed flat install provenance target could not be hashed: path={} source={err}",
                expected_installed_file.display()
            ),
            repair_command,
        )
    })?;
    if transform.installed_sha256 == expected_installed_sha256 {
        Ok(())
    } else {
        Err(reinstall_error(
            format!(
                "installed flat install provenance installed_sha256 mismatch for {artifact:?}: expected={} observed={}",
                expected_installed_sha256, transform.installed_sha256
            ),
            repair_command,
        ))
    }
}
