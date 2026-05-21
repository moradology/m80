use std::path::{Path, PathBuf};

use m80_image_manifest::{HostBinariesManifest, InstallProvenance, InstallProvenanceArtifact};
use serde::Serialize;

use crate::profile::RuntimeProfile;

use super::{diagnostic, InstallProfileReport, InstallStateDiagnostic, InstallStateDiagnosticCode};

mod bundle;
mod files;
mod proof_cache;

use files::{
    hash_file_nofollow, read_file_nofollow, reject_symlink_ancestors, reject_symlink_components,
    require_nonempty, require_sha256, sha256_bytes, validate_file_name, validate_relative_path,
    version_relative_path,
};

const BUNDLE_METADATA_NAME: &str = "bundle.json";
const INSTALL_PROVENANCE_NAME: &str = "install-provenance.json";
const HOST_BINARIES_MANIFEST_NAME: &str = "host-binaries.manifest.json";
const PROOF_CACHE_DIR: &str = "release-proof-cache";
const PROOF_CACHE_MANIFEST_NAME: &str = "manifest.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct InstallMetadataReport {
    pub(crate) version_dir: PathBuf,
    pub(crate) bundle_metadata: MetadataFileReport,
    pub(crate) install_provenance: MetadataFileReport,
    pub(crate) host_binaries_manifest: MetadataFileReport,
    pub(crate) proof_cache_manifest: MetadataFileReport,
    pub(crate) bundle: Option<BundleMetadataReport>,
    pub(crate) provenance: Option<InstallProvenanceReport>,
    pub(crate) host_binaries: Option<HostBinariesReport>,
    pub(crate) proof_cache: Option<ProofCacheReport>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct MetadataFileReport {
    pub(crate) path: PathBuf,
    pub(crate) status: MetadataFileStatus,
    pub(crate) sha256: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MetadataFileStatus {
    Present,
    Missing,
    Invalid,
    Stale,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct BundleMetadataReport {
    pub(crate) release_tag: String,
    pub(crate) m80_version: String,
    pub(crate) target: String,
    pub(crate) files: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct InstallProvenanceReport {
    pub(crate) release_tag: Option<String>,
    pub(crate) transforms: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct HostBinariesReport {
    pub(crate) schema_version: u32,
    pub(crate) binaries: usize,
    pub(crate) launch_material: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct ProofCacheReport {
    pub(crate) release_tag: String,
    pub(crate) repository: String,
    pub(crate) target: String,
    pub(crate) manifest_digest: String,
}

pub(super) fn read_install_metadata(
    profile: &RuntimeProfile,
    profile_report: &InstallProfileReport,
    diagnostics: &mut Vec<InstallStateDiagnostic>,
) -> InstallMetadataReport {
    let version_dir = profile_report
        .version_dir
        .clone()
        .expect("metadata reader is called only for installed profiles");
    let artifacts_dir = version_dir.join("artifacts");
    let bundle_path = version_dir.join(BUNDLE_METADATA_NAME);
    let provenance_path = profile
        .install_provenance
        .clone()
        .unwrap_or_else(|| artifacts_dir.join(INSTALL_PROVENANCE_NAME));
    let host_manifest_path = profile
        .host_binaries_manifest
        .clone()
        .unwrap_or_else(|| artifacts_dir.join(HOST_BINARIES_MANIFEST_NAME));
    let proof_cache_manifest_path = artifacts_dir
        .join(PROOF_CACHE_DIR)
        .join(PROOF_CACHE_MANIFEST_NAME);

    let (bundle_metadata, bundle) =
        bundle::read_bundle_metadata(&version_dir, &bundle_path, diagnostics);
    let (install_provenance, provenance) = read_install_provenance(
        &version_dir,
        &profile_report.release_tag,
        profile,
        &provenance_path,
        diagnostics,
    );
    let (host_binaries_manifest, host_binaries) =
        read_host_binaries_manifest(&version_dir, &host_manifest_path, diagnostics);
    let (proof_cache_manifest, proof_cache) = proof_cache::read_proof_cache_manifest(
        &version_dir,
        &proof_cache_manifest_path,
        diagnostics,
    );

    if let (Some(bundle), Some(provenance)) = (bundle.as_ref(), provenance.as_ref()) {
        if provenance.release_tag.as_deref() != Some(bundle.release_tag.as_str()) {
            diagnostics.push(diagnostic(
                InstallStateDiagnosticCode::InstallMetadataStale,
                Some("install_provenance.release_tag"),
                Some(provenance_path.clone()),
                format!(
                    "install provenance release_tag does not match bundle metadata: provenance={:?} bundle={}",
                    provenance.release_tag, bundle.release_tag
                ),
            ));
        }
    }
    if let (Some(bundle), Some(proof_cache)) = (bundle.as_ref(), proof_cache.as_ref()) {
        if proof_cache.release_tag != bundle.release_tag {
            diagnostics.push(diagnostic(
                InstallStateDiagnosticCode::ProofCacheStale,
                Some("proof_cache.release_tag"),
                Some(proof_cache_manifest_path.clone()),
                format!(
                    "proof cache release_tag does not match bundle metadata: proof={} bundle={}",
                    proof_cache.release_tag, bundle.release_tag
                ),
            ));
        }
    }

    InstallMetadataReport {
        version_dir,
        bundle_metadata,
        install_provenance,
        host_binaries_manifest,
        proof_cache_manifest,
        bundle,
        provenance,
        host_binaries,
        proof_cache,
    }
}

fn read_install_provenance(
    version_dir: &Path,
    expected_release_tag: &Option<String>,
    profile: &RuntimeProfile,
    path: &Path,
    diagnostics: &mut Vec<InstallStateDiagnostic>,
) -> (MetadataFileReport, Option<InstallProvenanceReport>) {
    let Some((raw, sha256)) =
        read_required_file(version_dir, path, "install_provenance", diagnostics)
    else {
        let status = if diagnostics.iter().any(|diagnostic| {
            diagnostic.path.as_deref() == Some(path)
                && diagnostic.code == InstallStateDiagnosticCode::InstallMetadataInvalid
        }) {
            MetadataFileStatus::Invalid
        } else {
            MetadataFileStatus::Missing
        };
        return (file_report(path, status, None), None);
    };
    let provenance = match InstallProvenance::from_bytes(&raw) {
        Ok(provenance) => provenance,
        Err(source) => {
            invalid_metadata(
                diagnostics,
                "install_provenance",
                path,
                format!("install provenance parse failed: {source}"),
            );
            return (
                file_report(path, MetadataFileStatus::Invalid, Some(sha256)),
                None,
            );
        }
    };
    if let Err(reason) =
        validate_install_provenance(version_dir, expected_release_tag, profile, &provenance)
    {
        stale_metadata(diagnostics, "install_provenance", path, reason);
        return (
            file_report(path, MetadataFileStatus::Stale, Some(sha256)),
            None,
        );
    }
    (
        file_report(path, MetadataFileStatus::Present, Some(sha256)),
        Some(InstallProvenanceReport {
            release_tag: provenance.release_tag,
            transforms: provenance.transforms.len(),
        }),
    )
}

fn read_host_binaries_manifest(
    version_dir: &Path,
    path: &Path,
    diagnostics: &mut Vec<InstallStateDiagnostic>,
) -> (MetadataFileReport, Option<HostBinariesReport>) {
    let Some((raw, sha256)) =
        read_required_file(version_dir, path, "host_binaries_manifest", diagnostics)
    else {
        let status = if diagnostics.iter().any(|diagnostic| {
            diagnostic.path.as_deref() == Some(path)
                && diagnostic.code == InstallStateDiagnosticCode::InstallMetadataInvalid
        }) {
            MetadataFileStatus::Invalid
        } else {
            MetadataFileStatus::Missing
        };
        return (file_report(path, status, None), None);
    };
    let manifest = match HostBinariesManifest::from_bytes(&raw) {
        Ok(manifest) => manifest,
        Err(source) => {
            invalid_metadata(
                diagnostics,
                "host_binaries_manifest",
                path,
                format!("host-binaries manifest parse failed: {source}"),
            );
            return (
                file_report(path, MetadataFileStatus::Invalid, Some(sha256)),
                None,
            );
        }
    };
    if let Err(reason) = validate_host_binaries_manifest(&manifest) {
        invalid_metadata(diagnostics, "host_binaries_manifest", path, reason);
        return (
            file_report(path, MetadataFileStatus::Invalid, Some(sha256)),
            None,
        );
    }
    (
        file_report(path, MetadataFileStatus::Present, Some(sha256)),
        Some(HostBinariesReport {
            schema_version: manifest.schema_version(),
            binaries: manifest.binaries.len(),
            launch_material: manifest.launch_material.len(),
        }),
    )
}

fn read_required_file(
    version_dir: &Path,
    path: &Path,
    field: &'static str,
    diagnostics: &mut Vec<InstallStateDiagnostic>,
) -> Option<(Vec<u8>, String)> {
    let relative_path = match version_relative_path(version_dir, path, field) {
        Ok(relative_path) => relative_path,
        Err(reason) => {
            invalid_metadata(diagnostics, field, path, reason);
            return None;
        }
    };
    if let Err(reason) = reject_symlink_ancestors(version_dir, &relative_path, field) {
        invalid_metadata(diagnostics, field, path, reason);
        return None;
    }
    match read_file_nofollow(path) {
        Ok(raw) => {
            let sha256 = sha256_bytes(&raw);
            Some((raw, sha256))
        }
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
            missing_metadata(diagnostics, field, path);
            None
        }
        Err(source) => {
            invalid_metadata(
                diagnostics,
                field,
                path,
                format!("metadata file is unreadable: {source}"),
            );
            None
        }
    }
}

fn validate_install_provenance(
    version_dir: &Path,
    expected_release_tag: &Option<String>,
    profile: &RuntimeProfile,
    provenance: &InstallProvenance,
) -> Result<(), String> {
    if let Some(expected) = expected_release_tag {
        if provenance.release_tag.as_deref() != Some(expected.as_str()) {
            return Err(format!(
                "install provenance release_tag mismatch: expected={expected} observed={:?}",
                provenance.release_tag
            ));
        }
    }
    for transform in &provenance.transforms {
        require_sha256("install_provenance.source_sha256", &transform.source_sha256)?;
        require_sha256(
            "install_provenance.installed_sha256",
            &transform.installed_sha256,
        )?;
        validate_relative_path(
            "install_provenance.source_path",
            &transform.source_path.display().to_string(),
        )?;
        let expected_path = match transform.artifact {
            InstallProvenanceArtifact::GuestManifest => profile.guest_manifest.as_deref(),
            InstallProvenanceArtifact::BuildReceipt => profile.build_receipt.as_deref(),
        };
        if let Some(expected_path) = expected_path {
            if transform.installed_path != expected_path {
                return Err(format!(
                    "install provenance installed_path mismatch for {:?}: expected={} observed={}",
                    transform.artifact,
                    expected_path.display(),
                    transform.installed_path.display()
                ));
            }
        }
        verify_version_path_digest(
            version_dir,
            &transform.installed_path,
            &transform.installed_sha256,
        )?;
    }
    Ok(())
}

fn validate_host_binaries_manifest(manifest: &HostBinariesManifest) -> Result<(), String> {
    if manifest.binaries.is_empty() {
        return Err("host-binaries manifest must list binaries".to_owned());
    }
    for binary in &manifest.binaries {
        if !binary.path.is_absolute() {
            return Err(format!(
                "host binary path must be absolute: name={} path={}",
                binary.name.as_str(),
                binary.path.display()
            ));
        }
        require_sha256("host_binaries.sha256", &binary.sha256)?;
        require_nonempty("host_binaries.version", &binary.version)?;
    }
    for material in &manifest.launch_material {
        if !material.path.is_absolute() {
            return Err(format!(
                "host launch material path must be absolute: name={} path={}",
                material.name.as_str(),
                material.path.display()
            ));
        }
        require_sha256("host_binaries.launch_material.sha256", &material.sha256)?;
        require_nonempty("host_binaries.launch_material.version", &material.version)?;
    }
    Ok(())
}

fn verify_version_path_digest(
    version_dir: &Path,
    path: &Path,
    expected_sha256: &str,
) -> Result<(), String> {
    let relative_path =
        version_relative_path(version_dir, path, "install_provenance.installed_path")?;
    reject_symlink_components(
        version_dir,
        &relative_path,
        "install_provenance.installed_path",
    )?;
    let (observed, _) = hash_file_nofollow(path).map_err(|source| {
        format!(
            "installed provenance target is unreadable: path={} source={source}",
            path.display()
        )
    })?;
    if observed != expected_sha256 {
        return Err(format!(
            "installed provenance target sha256 mismatch: path={} expected={} observed={observed}",
            path.display(),
            expected_sha256
        ));
    }
    Ok(())
}

fn file_report(
    path: &Path,
    status: MetadataFileStatus,
    sha256: Option<String>,
) -> MetadataFileReport {
    MetadataFileReport {
        path: path.to_path_buf(),
        status,
        sha256,
    }
}

fn missing_metadata(
    diagnostics: &mut Vec<InstallStateDiagnostic>,
    field: &'static str,
    path: &Path,
) {
    diagnostics.push(diagnostic(
        InstallStateDiagnosticCode::InstallMetadataMissing,
        Some(field),
        Some(path.to_path_buf()),
        format!("installed metadata file is missing: {}", path.display()),
    ));
}

fn invalid_metadata(
    diagnostics: &mut Vec<InstallStateDiagnostic>,
    field: &'static str,
    path: &Path,
    message: String,
) {
    diagnostics.push(diagnostic(
        InstallStateDiagnosticCode::InstallMetadataInvalid,
        Some(field),
        Some(path.to_path_buf()),
        message,
    ));
}

fn stale_metadata(
    diagnostics: &mut Vec<InstallStateDiagnostic>,
    field: &'static str,
    path: &Path,
    message: String,
) {
    diagnostics.push(diagnostic(
        InstallStateDiagnosticCode::InstallMetadataStale,
        Some(field),
        Some(path.to_path_buf()),
        message,
    ));
}

fn stale_proof_cache(diagnostics: &mut Vec<InstallStateDiagnostic>, path: &Path, message: String) {
    diagnostics.push(diagnostic(
        InstallStateDiagnosticCode::ProofCacheStale,
        Some("proof_cache_manifest"),
        Some(path.to_path_buf()),
        message,
    ));
}
