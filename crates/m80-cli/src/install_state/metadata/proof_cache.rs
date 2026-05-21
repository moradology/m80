use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::{
    age_seconds_at_now, file_report, hash_file_nofollow, modified_unix_seconds, read_file_nofollow,
    reject_symlink_ancestors, reject_symlink_components, require_nonempty, require_sha256,
    sha256_bytes, stale_proof_cache, validate_file_name, version_relative_path, MetadataFileReport,
    MetadataFileStatus, ProofCacheMaterialReport, ProofCacheReport, ProofCacheTrustPolicyReport,
    ProofCacheVerifierVersionsReport,
};
use crate::install_state::{diagnostic, InstallStateDiagnostic, InstallStateDiagnosticCode};

const PROOF_CACHE_SCHEMA_VERSION: u32 = 1;
const PROOF_CACHE_FILE_MODE: u32 = 0o644;
const PROOF_CACHE_DIR_MODE: u32 = 0o755;

pub(super) fn read_proof_cache_manifest(
    version_dir: &Path,
    path: &Path,
    diagnostics: &mut Vec<InstallStateDiagnostic>,
) -> (MetadataFileReport, Option<ProofCacheReport>) {
    let Some((raw, sha256)) = read_required_proof_file(version_dir, path, diagnostics) else {
        return (file_report(path, MetadataFileStatus::Missing, None), None);
    };
    let manifest = match serde_json::from_slice::<ProofCacheManifest>(&raw) {
        Ok(manifest) => manifest,
        Err(source) => {
            invalid_proof_cache(
                diagnostics,
                path,
                format!("proof-cache parse failed: {source}"),
            );
            return (
                file_report(path, MetadataFileStatus::Invalid, Some(sha256)),
                None,
            );
        }
    };
    let cache_dir = path
        .parent()
        .expect("proof-cache manifest should have parent");
    if let Err(reason) = verify_mode(cache_dir, PROOF_CACHE_DIR_MODE, "proof_cache.dir_mode") {
        stale_proof_cache(diagnostics, path, reason);
        return (
            file_report(path, MetadataFileStatus::Stale, Some(sha256)),
            None,
        );
    }
    if let Err(reason) = verify_mode(path, PROOF_CACHE_FILE_MODE, "proof_cache.manifest_mode") {
        stale_proof_cache(diagnostics, path, reason);
        return (
            file_report(path, MetadataFileStatus::Stale, Some(sha256)),
            None,
        );
    }
    if let Err(reason) = validate_proof_cache_manifest(cache_dir, &manifest) {
        stale_proof_cache(diagnostics, path, reason);
        return (
            file_report(path, MetadataFileStatus::Stale, Some(sha256)),
            None,
        );
    }
    (
        file_report(path, MetadataFileStatus::Present, Some(sha256)),
        Some(proof_cache_report(cache_dir, path, manifest)),
    )
}

fn proof_cache_report(
    cache_dir: &Path,
    manifest_path: &Path,
    manifest: ProofCacheManifest,
) -> ProofCacheReport {
    let manifest_modified_unix_seconds = modified_unix_seconds(manifest_path);
    let mut materials = vec![
        material_report(
            cache_dir,
            "integrity_predicate",
            &manifest.payload.integrity_predicate,
            None,
        ),
        material_report(
            cache_dir,
            "attestation_bundle",
            &manifest.payload.attestation_bundle,
            None,
        ),
        material_report(
            cache_dir,
            "attestation_metadata",
            &manifest.payload.attestation_metadata.file,
            None,
        ),
        material_report(
            cache_dir,
            "asset_index",
            &manifest.payload.asset_index,
            None,
        ),
        material_report(
            cache_dir,
            "public_sha256s",
            &manifest.payload.public_sha256s,
            None,
        ),
    ];
    materials.extend(manifest.payload.checksum_sidecars.iter().map(|sidecar| {
        ProofCacheMaterialReport {
            role: "checksum_sidecar".to_owned(),
            path: sidecar.path.clone(),
            sha256: sidecar.sha256.clone(),
            size_bytes: None,
            subject: Some(sidecar.subject.clone()),
            modified_unix_seconds: modified_unix_seconds(&cache_dir.join(&sidecar.path)),
        }
    }));
    ProofCacheReport {
        cache_dir: cache_dir.to_path_buf(),
        manifest_path: manifest_path.to_path_buf(),
        release_tag: manifest.payload.release_tag,
        repository: manifest.payload.repository,
        target: manifest.payload.target,
        manifest_digest: manifest.manifest_digest,
        manifest_modified_unix_seconds,
        cache_age_seconds: age_seconds_at_now(manifest_modified_unix_seconds),
        materials,
        trust_policy: ProofCacheTrustPolicyReport {
            path: manifest.payload.trust_policy.path.clone(),
            identity: manifest.payload.trust_policy.identity,
            sha256: manifest.payload.trust_policy.sha256.clone(),
            modified_unix_seconds: modified_unix_seconds(
                &cache_dir.join(&manifest.payload.trust_policy.path),
            ),
        },
        verifier_versions: ProofCacheVerifierVersionsReport {
            m80_version: manifest.payload.verifier_versions.m80_version,
            gh_version: manifest.payload.verifier_versions.gh_version,
            release_integrity_schema_version: manifest
                .payload
                .verifier_versions
                .release_integrity_schema_version,
            asset_index_schema_version: manifest
                .payload
                .verifier_versions
                .asset_index_schema_version,
        },
    }
}

fn material_report(
    cache_dir: &Path,
    role: &str,
    file: &ProofCacheFile,
    subject: Option<String>,
) -> ProofCacheMaterialReport {
    ProofCacheMaterialReport {
        role: role.to_owned(),
        path: file.path.clone(),
        sha256: file.sha256.clone(),
        size_bytes: Some(file.size_bytes),
        subject,
        modified_unix_seconds: modified_unix_seconds(&cache_dir.join(&file.path)),
    }
}

fn read_required_proof_file(
    version_dir: &Path,
    path: &Path,
    diagnostics: &mut Vec<InstallStateDiagnostic>,
) -> Option<(Vec<u8>, String)> {
    let relative_path = match version_relative_path(version_dir, path, "proof_cache_manifest") {
        Ok(relative_path) => relative_path,
        Err(reason) => {
            invalid_proof_cache(diagnostics, path, reason);
            return None;
        }
    };
    if let Err(reason) =
        reject_symlink_ancestors(version_dir, &relative_path, "proof_cache_manifest")
    {
        invalid_proof_cache(diagnostics, path, reason);
        return None;
    }
    match read_file_nofollow(path) {
        Ok(raw) => {
            let sha256 = sha256_bytes(&raw);
            Some((raw, sha256))
        }
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
            diagnostics.push(diagnostic(
                InstallStateDiagnosticCode::ProofCacheMissing,
                Some("proof_cache_manifest"),
                Some(path.to_path_buf()),
                "proof-cache manifest is missing".to_owned(),
            ));
            None
        }
        Err(source) => {
            invalid_proof_cache(
                diagnostics,
                path,
                format!("proof-cache manifest is unreadable: {source}"),
            );
            None
        }
    }
}

fn validate_proof_cache_manifest(
    cache_dir: &Path,
    manifest: &ProofCacheManifest,
) -> Result<(), String> {
    if manifest.schema_version != PROOF_CACHE_SCHEMA_VERSION {
        return Err(format!(
            "unsupported proof-cache schema_version: expected {PROOF_CACHE_SCHEMA_VERSION}, got {}",
            manifest.schema_version
        ));
    }
    require_sha256("proof_cache.manifest_digest", &manifest.manifest_digest)?;
    let observed = proof_cache_payload_digest(&manifest.payload)?;
    if observed != manifest.manifest_digest {
        return Err(format!(
            "proof-cache manifest_digest mismatch: expected={} observed={observed}",
            manifest.manifest_digest
        ));
    }
    require_nonempty("proof_cache.release_tag", &manifest.payload.release_tag)?;
    require_nonempty("proof_cache.repository", &manifest.payload.repository)?;
    require_nonempty("proof_cache.target", &manifest.payload.target)?;
    validate_proof_file(
        cache_dir,
        "integrity_predicate",
        &manifest.payload.integrity_predicate,
    )?;
    validate_proof_file(
        cache_dir,
        "attestation_bundle",
        &manifest.payload.attestation_bundle,
    )?;
    validate_proof_file(
        cache_dir,
        "attestation_metadata.file",
        &manifest.payload.attestation_metadata.file,
    )?;
    require_nonempty(
        "proof_cache.attestation_metadata.signer_identity",
        &manifest.payload.attestation_metadata.signer_identity,
    )?;
    require_nonempty(
        "proof_cache.attestation_metadata.issuer",
        &manifest.payload.attestation_metadata.issuer,
    )?;
    require_nonempty(
        "proof_cache.attestation_metadata.keyset_id",
        &manifest.payload.attestation_metadata.keyset_id,
    )?;
    require_sha256(
        "proof_cache.attestation_metadata.predicate_sha256",
        &manifest.payload.attestation_metadata.predicate_sha256,
    )?;
    validate_proof_file(cache_dir, "asset_index", &manifest.payload.asset_index)?;
    validate_proof_file(
        cache_dir,
        "public_sha256s",
        &manifest.payload.public_sha256s,
    )?;
    if manifest.payload.checksum_sidecars.is_empty() {
        return Err("proof-cache checksum_sidecars must not be empty".to_owned());
    }
    for sidecar in &manifest.payload.checksum_sidecars {
        validate_file_name("proof_cache.checksum_sidecars.path", &sidecar.path)?;
        require_sha256("proof_cache.checksum_sidecars.sha256", &sidecar.sha256)?;
        require_nonempty("proof_cache.checksum_sidecars.subject", &sidecar.subject)?;
        verify_cache_file(cache_dir, &sidecar.path, &sidecar.sha256, None)?;
    }
    validate_file_name(
        "proof_cache.trust_policy.path",
        &manifest.payload.trust_policy.path,
    )?;
    require_nonempty(
        "proof_cache.trust_policy.identity",
        &manifest.payload.trust_policy.identity,
    )?;
    require_sha256(
        "proof_cache.trust_policy.sha256",
        &manifest.payload.trust_policy.sha256,
    )?;
    verify_cache_file(
        cache_dir,
        &manifest.payload.trust_policy.path,
        &manifest.payload.trust_policy.sha256,
        None,
    )?;
    require_nonempty(
        "proof_cache.verifier_versions.m80_version",
        &manifest.payload.verifier_versions.m80_version,
    )?;
    require_nonempty(
        "proof_cache.verifier_versions.gh_version",
        &manifest.payload.verifier_versions.gh_version,
    )?;
    if manifest
        .payload
        .verifier_versions
        .release_integrity_schema_version
        == 0
    {
        return Err(
            "proof-cache verifier_versions.release_integrity_schema_version must be nonzero"
                .to_owned(),
        );
    }
    if manifest
        .payload
        .verifier_versions
        .asset_index_schema_version
        == 0
    {
        return Err(
            "proof-cache verifier_versions.asset_index_schema_version must be nonzero".to_owned(),
        );
    }
    Ok(())
}

fn validate_proof_file(
    cache_dir: &Path,
    label: &'static str,
    file: &ProofCacheFile,
) -> Result<(), String> {
    validate_file_name(&format!("proof_cache.{label}.path"), &file.path)?;
    require_sha256(&format!("proof_cache.{label}.sha256"), &file.sha256)?;
    if file.size_bytes == 0 {
        return Err(format!("proof-cache {label}.size_bytes must be nonzero"));
    }
    verify_cache_file(cache_dir, &file.path, &file.sha256, Some(file.size_bytes))
}

fn verify_cache_file(
    cache_dir: &Path,
    relative_path: &str,
    expected_sha256: &str,
    expected_size: Option<u64>,
) -> Result<(), String> {
    validate_file_name("proof_cache.path", relative_path)?;
    let path = cache_dir.join(relative_path);
    let cache_relative_path =
        version_relative_path(cache_dir, &path, "proof_cache.path").map_err(|source| {
            format!(
                "proof-cache referenced file path is invalid: path={} source={source}",
                path.display()
            )
        })?;
    reject_symlink_components(cache_dir, &cache_relative_path, "proof_cache.path")?;
    let (observed, observed_size) = hash_file_nofollow(&path).map_err(|source| {
        format!(
            "proof-cache referenced file is unreadable: path={} source={source}",
            path.display()
        )
    })?;
    if let Some(expected_size) = expected_size {
        if observed_size != expected_size {
            return Err(format!(
                "proof-cache referenced file size mismatch: path={} expected={} observed={}",
                path.display(),
                expected_size,
                observed_size
            ));
        }
    }
    if observed != expected_sha256 {
        return Err(format!(
            "proof-cache referenced file sha256 mismatch: path={} expected={} observed={observed}",
            path.display(),
            expected_sha256
        ));
    }
    verify_mode(&path, PROOF_CACHE_FILE_MODE, "proof_cache.file_mode")?;
    Ok(())
}

fn verify_mode(path: &Path, expected: u32, field: &'static str) -> Result<(), String> {
    let observed = std::fs::symlink_metadata(path)
        .map_err(|source| {
            format!(
                "proof-cache mode check failed: field={field} path={} source={source}",
                path.display()
            )
        })?
        .permissions()
        .mode()
        & 0o777;
    if observed == expected {
        Ok(())
    } else {
        Err(format!(
            "proof-cache mode mismatch: field={field} path={} expected={expected:o} observed={observed:o}",
            path.display()
        ))
    }
}

fn proof_cache_payload_digest(payload: &ProofCachePayload) -> Result<String, String> {
    serde_json::to_vec(payload)
        .map(|encoded| sha256_bytes(&encoded))
        .map_err(|source| format!("proof-cache payload digest encode failed: {source}"))
}

fn invalid_proof_cache(
    diagnostics: &mut Vec<InstallStateDiagnostic>,
    path: &Path,
    message: String,
) {
    diagnostics.push(diagnostic(
        InstallStateDiagnosticCode::ProofCacheInvalid,
        Some("proof_cache_manifest"),
        Some(path.to_path_buf()),
        message,
    ));
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProofCacheManifest {
    schema_version: u32,
    manifest_digest: String,
    payload: ProofCachePayload,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProofCachePayload {
    release_tag: String,
    repository: String,
    target: String,
    integrity_predicate: ProofCacheFile,
    attestation_bundle: ProofCacheFile,
    attestation_metadata: AttestationMetadataRef,
    asset_index: ProofCacheFile,
    public_sha256s: ProofCacheFile,
    checksum_sidecars: Vec<ChecksumSidecarRef>,
    trust_policy: TrustPolicyRef,
    verifier_versions: VerifierVersions,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProofCacheFile {
    path: String,
    sha256: String,
    size_bytes: u64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AttestationMetadataRef {
    file: ProofCacheFile,
    signer_identity: String,
    issuer: String,
    keyset_id: String,
    predicate_sha256: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ChecksumSidecarRef {
    path: String,
    sha256: String,
    subject: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TrustPolicyRef {
    path: String,
    identity: String,
    sha256: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct VerifierVersions {
    m80_version: String,
    gh_version: String,
    release_integrity_schema_version: u32,
    asset_index_schema_version: u32,
}
