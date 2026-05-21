use std::collections::BTreeSet;
use std::path::Path;

use serde::Deserialize;

use super::{
    file_report, hash_file_nofollow, invalid_metadata, read_required_file,
    reject_symlink_components, require_nonempty, require_sha256, stale_metadata,
    validate_relative_path, version_relative_path, BundleMetadataReport, MetadataFileReport,
    MetadataFileStatus,
};
use crate::install_state::InstallStateDiagnostic;

const BUNDLE_SCHEMA_VERSION: u32 = 1;

pub(super) fn read_bundle_metadata(
    version_dir: &Path,
    path: &Path,
    diagnostics: &mut Vec<InstallStateDiagnostic>,
) -> (MetadataFileReport, Option<BundleMetadataReport>) {
    let Some((raw, sha256)) = read_required_file(version_dir, path, "bundle_metadata", diagnostics)
    else {
        return (file_report(path, MetadataFileStatus::Missing, None), None);
    };
    let parsed = match serde_json::from_slice::<InstalledBundleMetadata>(&raw) {
        Ok(parsed) => parsed,
        Err(source) => {
            invalid_metadata(
                diagnostics,
                "bundle_metadata",
                path,
                format!("bundle metadata parse failed: {source}"),
            );
            return (
                file_report(path, MetadataFileStatus::Invalid, Some(sha256)),
                None,
            );
        }
    };
    if let Err(reason) = validate_bundle_metadata(version_dir, &parsed) {
        invalid_metadata(diagnostics, "bundle_metadata", path, reason);
        return (
            file_report(path, MetadataFileStatus::Invalid, Some(sha256)),
            None,
        );
    }
    if let Err(reason) = verify_bundle_file_refs(version_dir, &parsed) {
        stale_metadata(diagnostics, "bundle_metadata", path, reason);
        return (
            file_report(path, MetadataFileStatus::Stale, Some(sha256)),
            None,
        );
    }
    (
        file_report(path, MetadataFileStatus::Present, Some(sha256)),
        Some(BundleMetadataReport {
            release_tag: parsed.release_tag,
            m80_version: parsed.m80_version,
            target: parsed.target,
            files: parsed.files.len(),
        }),
    )
}

fn validate_bundle_metadata(
    version_dir: &Path,
    metadata: &InstalledBundleMetadata,
) -> Result<(), String> {
    if metadata.schema_version != BUNDLE_SCHEMA_VERSION {
        return Err(format!(
            "unsupported bundle metadata schema_version: expected {BUNDLE_SCHEMA_VERSION}, got {}",
            metadata.schema_version
        ));
    }
    for (field, value) in [
        ("release_tag", metadata.release_tag.as_str()),
        ("m80_version", metadata.m80_version.as_str()),
        ("target", metadata.target.as_str()),
    ] {
        require_nonempty(field, value)?;
    }
    if metadata.files.is_empty() {
        return Err("bundle metadata files must not be empty".to_owned());
    }
    let mut seen = BTreeSet::new();
    for file in &metadata.files {
        validate_relative_path("bundle.files.path", &file.path)?;
        require_sha256("bundle.files.sha256", &file.sha256)?;
        if file.size_bytes == 0 {
            return Err(format!(
                "bundle metadata file {} has zero size_bytes",
                file.path
            ));
        }
        if !seen.insert(file.path.as_str()) {
            return Err(format!(
                "duplicate bundle metadata file path: {}",
                file.path
            ));
        }
        let installed = version_dir.join(&file.path);
        if !installed.starts_with(version_dir) {
            return Err(format!(
                "bundle metadata file escapes version dir: {}",
                file.path
            ));
        }
    }
    Ok(())
}

fn verify_bundle_file_refs(
    version_dir: &Path,
    metadata: &InstalledBundleMetadata,
) -> Result<(), String> {
    for file in &metadata.files {
        let path = version_dir.join(&file.path);
        let relative_path = version_relative_path(version_dir, &path, "bundle.files.path")?;
        reject_symlink_components(version_dir, &relative_path, "bundle.files.path")?;
        let (observed, observed_size) = hash_file_nofollow(&path).map_err(|source| {
            format!(
                "referenced bundle file is unreadable: path={} source={source}",
                path.display()
            )
        })?;
        if observed_size != file.size_bytes {
            return Err(format!(
                "referenced bundle file size mismatch: path={} expected={} observed={}",
                path.display(),
                file.size_bytes,
                observed_size
            ));
        }
        if observed != file.sha256 {
            return Err(format!(
                "referenced bundle file sha256 mismatch: path={} expected={} observed={observed}",
                path.display(),
                file.sha256
            ));
        }
    }
    Ok(())
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct InstalledBundleMetadata {
    schema_version: u32,
    release_tag: String,
    m80_version: String,
    package_version: String,
    target: String,
    os: String,
    arch: String,
    image_kind: String,
    m80_protocol_version: u32,
    guestd_package_version: String,
    guest_protocol_version: u32,
    manifest_schema_version: u32,
    build_receipt_schema_version: u32,
    build_receipt_manifest_path: String,
    install_provenance_schema_version: u32,
    install_provenance_required: bool,
    expected_firecracker_version: String,
    files: Vec<InstalledBundleFile>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct InstalledBundleFile {
    path: String,
    sha256: String,
    size_bytes: u64,
}
