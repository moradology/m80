use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use m80_firecracker::{ConfigError, FcError};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::release_material::VerifiedOfficialReleaseBundle;

mod reinstall;

pub(super) use reinstall::compare_existing_release_proof_cache;
pub(crate) use reinstall::ProofCacheReinstallReport;

pub(super) const PROOF_CACHE_DIR: &str = "release-proof-cache";
pub(super) const PROOF_CACHE_MANIFEST: &str = "manifest.json";
pub(super) const PROOF_CACHE_SCHEMA_VERSION: u32 = 1;

const PROOF_CACHE_FILE_MODE: u32 = 0o644;
const PROOF_CACHE_DIR_MODE: u32 = 0o755;
const TRUST_POLICY_NAME: &str = "m80-release-trust-policy.json";
const TRUST_POLICY_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../docs/behaviors/release/m80-release-trust-policy.json"
));
const CHECKSUM_SIDECAR_MATERIALS: &[&str] = &[
    "bundle-checksum",
    "bundle-metadata-checksum",
    "asset-index-checksum",
    "install-script-checksum",
    "bootstrap-selector-checksum",
    "release-build-checksum",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ProofCacheManifest {
    pub(super) schema_version: u32,
    pub(super) manifest_digest: String,
    pub(super) payload: ProofCachePayload,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ProofCachePayload {
    pub(super) release_tag: String,
    pub(super) repository: String,
    pub(super) target: String,
    pub(super) integrity_predicate: ProofCacheFile,
    pub(super) attestation_bundle: ProofCacheFile,
    pub(super) attestation_metadata: AttestationMetadataRef,
    pub(super) asset_index: ProofCacheFile,
    pub(super) public_sha256s: ProofCacheFile,
    pub(super) checksum_sidecars: Vec<ChecksumSidecarRef>,
    pub(super) trust_policy: TrustPolicyRef,
    pub(super) verifier_versions: VerifierVersions,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ProofCacheFile {
    pub(super) path: String,
    pub(super) sha256: String,
    pub(super) size_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AttestationMetadataRef {
    pub(super) file: ProofCacheFile,
    pub(super) signer_identity: String,
    pub(super) issuer: String,
    pub(super) keyset_id: String,
    pub(super) predicate_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ChecksumSidecarRef {
    pub(super) path: String,
    pub(super) sha256: String,
    pub(super) subject: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct TrustPolicyRef {
    pub(super) path: String,
    pub(super) identity: String,
    pub(super) sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct VerifierVersions {
    pub(super) m80_version: String,
    pub(super) attestation_verifier: String,
    pub(super) release_integrity_schema_version: u32,
    pub(super) asset_index_schema_version: u32,
}

struct SavedFile {
    descriptor: ProofCacheFile,
    cache_path: PathBuf,
}

pub(super) fn write_verified_release_proof_cache(
    verified: &VerifiedOfficialReleaseBundle,
    final_dir: &Path,
    m80_version: &str,
) -> Result<PathBuf, FcError> {
    let manifest = verified_release_proof_cache_manifest(verified, m80_version)?;
    let cache_dir = final_dir.join("artifacts").join(PROOF_CACHE_DIR);
    fs::create_dir(&cache_dir).map_err(|source| FcError::PathIo {
        path: cache_dir.clone(),
        source,
    })?;
    fs::set_permissions(&cache_dir, fs::Permissions::from_mode(PROOF_CACHE_DIR_MODE)).map_err(
        |source| FcError::PathIo {
            path: cache_dir.clone(),
            source,
        },
    )?;
    maybe_inject_write_failure()?;

    let mut saved_paths = Vec::new();
    saved_paths.push(copy_expected_material(
        verified,
        &cache_dir,
        "release-integrity-predicate",
        &manifest.payload.integrity_predicate,
    )?);
    saved_paths.push(copy_expected_material(
        verified,
        &cache_dir,
        "release-attestation-bundle",
        &manifest.payload.attestation_bundle,
    )?);
    saved_paths.push(copy_expected_material(
        verified,
        &cache_dir,
        "release-attestation-metadata",
        &manifest.payload.attestation_metadata.file,
    )?);
    saved_paths.push(copy_expected_material(
        verified,
        &cache_dir,
        "asset-index",
        &manifest.payload.asset_index,
    )?);
    saved_paths.push(copy_expected_material(
        verified,
        &cache_dir,
        "public-sha256s",
        &manifest.payload.public_sha256s,
    )?);
    for (class, sidecar) in CHECKSUM_SIDECAR_MATERIALS
        .iter()
        .zip(&manifest.payload.checksum_sidecars)
    {
        saved_paths.push(copy_expected_material_with_subject(
            verified, &cache_dir, class, sidecar,
        )?);
    }
    let trust_policy = write_trust_policy(&cache_dir, verified)?;
    if trust_policy.descriptor != manifest.payload.trust_policy {
        return Err(invalid_manifest(
            "internal proof-cache trust policy descriptor mismatch".to_owned(),
        ));
    }
    saved_paths.push(trust_policy.cache_path.clone());
    let manifest = if inject_manifest_digest_failure() {
        ProofCacheManifest {
            manifest_digest: "0".repeat(64),
            ..manifest
        }
    } else {
        manifest
    };
    let manifest_path = cache_dir.join(PROOF_CACHE_MANIFEST);
    write_json_file(&manifest_path, &manifest)?;
    saved_paths.push(manifest_path.clone());

    maybe_inject_mode_failure(&saved_paths)?;
    let parsed = read_proof_cache_manifest(&manifest_path)?;
    verify_cached_payload_files(&cache_dir, &parsed.payload)?;
    verify_dir_mode(&cache_dir)?;
    for path in &saved_paths {
        verify_file_mode(path)?;
    }
    Ok(manifest_path)
}

fn verified_release_proof_cache_manifest(
    verified: &VerifiedOfficialReleaseBundle,
    m80_version: &str,
) -> Result<ProofCacheManifest, FcError> {
    let integrity_predicate = describe_material(
        verified,
        "release-integrity-predicate",
        "m80-release-integrity.json",
    )?;
    let attestation_bundle = describe_material(
        verified,
        "release-attestation-bundle",
        "m80-release-integrity.attestation.jsonl",
    )?;
    let attestation_metadata = describe_material(
        verified,
        "release-attestation-metadata",
        "m80-release-attestation.json",
    )?;
    let asset_index = describe_material(verified, "asset-index", "m80-release-assets.json")?;
    let public_sha256s = describe_material(verified, "public-sha256s", "SHA256SUMS")?;
    let checksum_sidecars = CHECKSUM_SIDECAR_MATERIALS
        .iter()
        .map(|class| {
            let descriptor = describe_material_with_source_name(verified, class)?;
            let subject = checksum_sidecar_subject(verified.material_path(class)?)?;
            Ok(ChecksumSidecarRef {
                path: descriptor.path,
                sha256: descriptor.sha256,
                subject,
            })
        })
        .collect::<Result<Vec<_>, FcError>>()?;
    let payload = ProofCachePayload {
        release_tag: verified.summary.release_tag.clone(),
        repository: verified.summary.repository.clone(),
        target: verified.summary.target.clone(),
        integrity_predicate,
        attestation_bundle,
        attestation_metadata: AttestationMetadataRef {
            file: attestation_metadata,
            signer_identity: verified.summary.attestation_signer.clone(),
            issuer: verified.summary.attestation_issuer.clone(),
            keyset_id: verified.summary.attestation_keyset_id.clone(),
            predicate_sha256: verified.summary.predicate_sha256.clone(),
        },
        asset_index,
        public_sha256s,
        checksum_sidecars,
        trust_policy: trust_policy_ref(verified),
        verifier_versions: VerifierVersions {
            m80_version: m80_version.to_owned(),
            attestation_verifier: attestation_verifier()?,
            release_integrity_schema_version: verified.summary.release_integrity_schema_version,
            asset_index_schema_version: crate::release_asset_index::ASSET_INDEX_SCHEMA_VERSION,
        },
    };
    Ok(ProofCacheManifest {
        schema_version: PROOF_CACHE_SCHEMA_VERSION,
        manifest_digest: proof_cache_manifest_digest(&payload)?,
        payload,
    })
}

pub(super) fn read_proof_cache_manifest(path: &Path) -> Result<ProofCacheManifest, FcError> {
    let raw = fs::read(path).map_err(|source| FcError::PathIo {
        path: path.to_path_buf(),
        source,
    })?;
    let manifest: ProofCacheManifest =
        serde_json::from_slice(&raw).map_err(|source| FcError::Json {
            context: "read proof cache manifest",
            source,
        })?;
    validate_manifest(&manifest)?;
    Ok(manifest)
}

pub(super) fn proof_cache_manifest_digest(payload: &ProofCachePayload) -> Result<String, FcError> {
    let encoded = serde_json::to_vec(payload).map_err(|source| FcError::Json {
        context: "digest proof cache manifest payload",
        source,
    })?;
    Ok(format!("{:x}", Sha256::digest(encoded)))
}

fn validate_manifest(manifest: &ProofCacheManifest) -> Result<(), FcError> {
    if manifest.schema_version != PROOF_CACHE_SCHEMA_VERSION {
        return Err(invalid_manifest(format!(
            "unsupported proof-cache schema_version: expected {PROOF_CACHE_SCHEMA_VERSION}, got {}",
            manifest.schema_version
        )));
    }
    require_sha256("manifest_digest", &manifest.manifest_digest)?;
    validate_payload(&manifest.payload)?;
    let observed = proof_cache_manifest_digest(&manifest.payload)?;
    if observed != manifest.manifest_digest {
        return Err(invalid_manifest(format!(
            "manifest_digest mismatch: expected {}, got {observed}",
            manifest.manifest_digest
        )));
    }
    Ok(())
}

fn validate_payload(payload: &ProofCachePayload) -> Result<(), FcError> {
    require_nonempty("release_tag", &payload.release_tag)?;
    require_nonempty("repository", &payload.repository)?;
    require_nonempty("target", &payload.target)?;
    validate_file("integrity_predicate", &payload.integrity_predicate)?;
    validate_file("attestation_bundle", &payload.attestation_bundle)?;
    validate_file(
        "attestation_metadata.file",
        &payload.attestation_metadata.file,
    )?;
    require_nonempty(
        "attestation_metadata.signer_identity",
        &payload.attestation_metadata.signer_identity,
    )?;
    require_nonempty(
        "attestation_metadata.issuer",
        &payload.attestation_metadata.issuer,
    )?;
    require_nonempty(
        "attestation_metadata.keyset_id",
        &payload.attestation_metadata.keyset_id,
    )?;
    require_sha256(
        "attestation_metadata.predicate_sha256",
        &payload.attestation_metadata.predicate_sha256,
    )?;
    validate_file("asset_index", &payload.asset_index)?;
    validate_file("public_sha256s", &payload.public_sha256s)?;
    if payload.checksum_sidecars.is_empty() {
        return Err(invalid_manifest(
            "checksum_sidecars must contain at least one sidecar".to_owned(),
        ));
    }
    for sidecar in &payload.checksum_sidecars {
        validate_checksum_sidecar(sidecar)?;
    }
    require_nonempty("trust_policy.path", &payload.trust_policy.path)?;
    require_nonempty("trust_policy.identity", &payload.trust_policy.identity)?;
    require_sha256("trust_policy.sha256", &payload.trust_policy.sha256)?;
    require_nonempty(
        "verifier_versions.m80_version",
        &payload.verifier_versions.m80_version,
    )?;
    require_nonempty(
        "verifier_versions.attestation_verifier",
        &payload.verifier_versions.attestation_verifier,
    )?;
    if payload.verifier_versions.release_integrity_schema_version == 0 {
        return Err(invalid_manifest(
            "verifier_versions.release_integrity_schema_version must be nonzero".to_owned(),
        ));
    }
    if payload.verifier_versions.asset_index_schema_version == 0 {
        return Err(invalid_manifest(
            "verifier_versions.asset_index_schema_version must be nonzero".to_owned(),
        ));
    }
    Ok(())
}

fn validate_file(label: &str, file: &ProofCacheFile) -> Result<(), FcError> {
    require_nonempty(&format!("{label}.path"), &file.path)?;
    require_sha256(&format!("{label}.sha256"), &file.sha256)?;
    if file.size_bytes == 0 {
        return Err(invalid_manifest(format!(
            "{label}.size_bytes must be nonzero"
        )));
    }
    Ok(())
}

fn validate_checksum_sidecar(sidecar: &ChecksumSidecarRef) -> Result<(), FcError> {
    require_nonempty("checksum_sidecars.path", &sidecar.path)?;
    require_sha256("checksum_sidecars.sha256", &sidecar.sha256)?;
    require_nonempty("checksum_sidecars.subject", &sidecar.subject)
}

fn copy_material(
    verified: &VerifiedOfficialReleaseBundle,
    cache_dir: &Path,
    class: &'static str,
    name: &str,
) -> Result<SavedFile, FcError> {
    let source = verified.material_path(class)?;
    let dest = cache_dir.join(name);
    fs::copy(source, &dest).map_err(|source| FcError::PathIo {
        path: dest.clone(),
        source,
    })?;
    fs::set_permissions(&dest, fs::Permissions::from_mode(PROOF_CACHE_FILE_MODE)).map_err(
        |source| FcError::PathIo {
            path: dest.clone(),
            source,
        },
    )?;
    Ok(SavedFile {
        descriptor: describe_file(name, &dest)?,
        cache_path: dest,
    })
}

fn copy_expected_material(
    verified: &VerifiedOfficialReleaseBundle,
    cache_dir: &Path,
    class: &'static str,
    expected: &ProofCacheFile,
) -> Result<PathBuf, FcError> {
    let saved = copy_material(verified, cache_dir, class, &expected.path)?;
    if &saved.descriptor != expected {
        return Err(invalid_manifest(format!(
            "internal proof-cache descriptor mismatch: material_class={class}"
        )));
    }
    Ok(saved.cache_path)
}

fn copy_material_with_source_name(
    verified: &VerifiedOfficialReleaseBundle,
    cache_dir: &Path,
    class: &'static str,
) -> Result<SavedFile, FcError> {
    let source = verified.material_path(class)?;
    let name = source
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            invalid_manifest(format!(
                "proof-cache material path has no UTF-8 file name: material_class={class} path={}",
                source.display()
            ))
        })?
        .to_owned();
    copy_material(verified, cache_dir, class, &name)
}

fn copy_expected_material_with_subject(
    verified: &VerifiedOfficialReleaseBundle,
    cache_dir: &Path,
    class: &'static str,
    expected: &ChecksumSidecarRef,
) -> Result<PathBuf, FcError> {
    let saved = copy_material_with_source_name(verified, cache_dir, class)?;
    let subject = checksum_sidecar_subject(&saved.cache_path)?;
    let observed = ChecksumSidecarRef {
        path: saved.descriptor.path,
        sha256: saved.descriptor.sha256,
        subject,
    };
    if &observed != expected {
        return Err(invalid_manifest(format!(
            "internal proof-cache checksum sidecar mismatch: material_class={class}"
        )));
    }
    Ok(saved.cache_path)
}

fn describe_material(
    verified: &VerifiedOfficialReleaseBundle,
    class: &'static str,
    name: &str,
) -> Result<ProofCacheFile, FcError> {
    describe_file(name, verified.material_path(class)?)
}

fn describe_material_with_source_name(
    verified: &VerifiedOfficialReleaseBundle,
    class: &'static str,
) -> Result<ProofCacheFile, FcError> {
    let source = verified.material_path(class)?;
    let name = source
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            invalid_manifest(format!(
                "proof-cache material path has no UTF-8 file name: material_class={class} path={}",
                source.display()
            ))
        })?;
    describe_file(name, source)
}

struct SavedTrustPolicy {
    descriptor: TrustPolicyRef,
    cache_path: PathBuf,
}

fn trust_policy_ref(verified: &VerifiedOfficialReleaseBundle) -> TrustPolicyRef {
    TrustPolicyRef {
        path: TRUST_POLICY_NAME.to_owned(),
        identity: format!(
            "repository={} signer={} issuer={} keyset_id={}",
            verified.summary.repository,
            verified.summary.attestation_signer,
            verified.summary.attestation_issuer,
            verified.summary.attestation_keyset_id
        ),
        sha256: sha256_bytes(TRUST_POLICY_BYTES),
    }
}

fn write_trust_policy(
    cache_dir: &Path,
    verified: &VerifiedOfficialReleaseBundle,
) -> Result<SavedTrustPolicy, FcError> {
    let path = cache_dir.join(TRUST_POLICY_NAME);
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|source| FcError::PathIo {
            path: path.clone(),
            source,
        })?;
    file.write_all(TRUST_POLICY_BYTES)
        .map_err(|source| FcError::PathIo {
            path: path.clone(),
            source,
        })?;
    fs::set_permissions(&path, fs::Permissions::from_mode(PROOF_CACHE_FILE_MODE)).map_err(
        |source| FcError::PathIo {
            path: path.clone(),
            source,
        },
    )?;
    Ok(SavedTrustPolicy {
        descriptor: trust_policy_ref(verified),
        cache_path: path,
    })
}

fn write_json_file<T: Serialize>(path: &Path, value: &T) -> Result<(), FcError> {
    let encoded = serde_json::to_vec_pretty(value).map_err(|source| FcError::Json {
        context: "write proof cache manifest",
        source,
    })?;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|source| FcError::PathIo {
            path: path.to_path_buf(),
            source,
        })?;
    file.write_all(&encoded)
        .and_then(|_| file.write_all(b"\n"))
        .map_err(|source| FcError::PathIo {
            path: path.to_path_buf(),
            source,
        })?;
    fs::set_permissions(path, fs::Permissions::from_mode(PROOF_CACHE_FILE_MODE)).map_err(|source| {
        FcError::PathIo {
            path: path.to_path_buf(),
            source,
        }
    })
}

fn describe_file(relative_path: &str, path: &Path) -> Result<ProofCacheFile, FcError> {
    let metadata = fs::metadata(path).map_err(|source| FcError::PathIo {
        path: path.to_path_buf(),
        source,
    })?;
    if !metadata.is_file() {
        return Err(invalid_manifest(format!(
            "proof-cache material is not a regular file: {}",
            path.display()
        )));
    }
    Ok(ProofCacheFile {
        path: relative_path.to_owned(),
        sha256: sha256_file(path)?,
        size_bytes: metadata.len(),
    })
}

fn checksum_sidecar_subject(path: &Path) -> Result<String, FcError> {
    let text = fs::read_to_string(path).map_err(|source| FcError::PathIo {
        path: path.to_path_buf(),
        source,
    })?;
    let mut parts = text.split_whitespace();
    let _digest = parts.next().ok_or_else(|| {
        invalid_manifest(format!(
            "checksum sidecar is empty: path={}",
            path.display()
        ))
    })?;
    let subject = parts.next().ok_or_else(|| {
        invalid_manifest(format!(
            "checksum sidecar missing subject: path={}",
            path.display()
        ))
    })?;
    require_nonempty("checksum_sidecars.subject", subject)?;
    Ok(subject.to_owned())
}

fn attestation_verifier() -> Result<String, FcError> {
    Ok("m80 native release-attestation verifier v1".to_owned())
}

fn verify_dir_mode(path: &Path) -> Result<(), FcError> {
    verify_mode(path, PROOF_CACHE_DIR_MODE, "proof-cache.dir-mode")
}

fn verify_file_mode(path: &Path) -> Result<(), FcError> {
    verify_mode(path, PROOF_CACHE_FILE_MODE, "proof-cache.file-mode")
}

fn verify_cached_payload_files(
    cache_dir: &Path,
    payload: &ProofCachePayload,
) -> Result<(), FcError> {
    verify_proof_file(
        cache_dir,
        "integrity_predicate",
        &payload.integrity_predicate,
    )?;
    verify_proof_file(cache_dir, "attestation_bundle", &payload.attestation_bundle)?;
    verify_proof_file(
        cache_dir,
        "attestation_metadata.file",
        &payload.attestation_metadata.file,
    )?;
    verify_proof_file(cache_dir, "asset_index", &payload.asset_index)?;
    verify_proof_file(cache_dir, "public_sha256s", &payload.public_sha256s)?;
    for sidecar in &payload.checksum_sidecars {
        verify_sha_ref(
            cache_dir,
            "checksum_sidecars",
            &sidecar.path,
            &sidecar.sha256,
        )?;
    }
    verify_sha_ref(
        cache_dir,
        "trust_policy",
        &payload.trust_policy.path,
        &payload.trust_policy.sha256,
    )
}

fn verify_payload_file_modes(cache_dir: &Path, payload: &ProofCachePayload) -> Result<(), FcError> {
    for path in proof_cache_payload_paths(cache_dir, payload)? {
        verify_file_mode(&path)?;
    }
    Ok(())
}

fn proof_cache_payload_paths(
    cache_dir: &Path,
    payload: &ProofCachePayload,
) -> Result<Vec<PathBuf>, FcError> {
    let mut paths = vec![
        cache_file_path(cache_dir, &payload.integrity_predicate.path)?,
        cache_file_path(cache_dir, &payload.attestation_bundle.path)?,
        cache_file_path(cache_dir, &payload.attestation_metadata.file.path)?,
        cache_file_path(cache_dir, &payload.asset_index.path)?,
        cache_file_path(cache_dir, &payload.public_sha256s.path)?,
    ];
    for sidecar in &payload.checksum_sidecars {
        paths.push(cache_file_path(cache_dir, &sidecar.path)?);
    }
    paths.push(cache_file_path(cache_dir, &payload.trust_policy.path)?);
    Ok(paths)
}

fn verify_proof_file(
    cache_dir: &Path,
    label: &'static str,
    descriptor: &ProofCacheFile,
) -> Result<(), FcError> {
    let path = cache_file_path(cache_dir, &descriptor.path)?;
    let metadata = fs::metadata(&path).map_err(|source| FcError::PathIo {
        path: path.clone(),
        source,
    })?;
    if !metadata.is_file() {
        return Err(invalid_cached_file(format!(
            "{label} is not a regular file: path={}",
            path.display()
        )));
    }
    if metadata.len() != descriptor.size_bytes {
        return Err(invalid_cached_file(format!(
            "{label} size mismatch: path={} expected_size_bytes={} observed_size_bytes={}",
            path.display(),
            descriptor.size_bytes,
            metadata.len()
        )));
    }
    let observed = sha256_file(&path)?;
    if observed != descriptor.sha256 {
        return Err(invalid_cached_file(format!(
            "{label} sha256 mismatch: path={} expected_sha256={} observed_sha256={observed}",
            path.display(),
            descriptor.sha256
        )));
    }
    Ok(())
}

fn verify_sha_ref(
    cache_dir: &Path,
    label: &'static str,
    relative_path: &str,
    expected_sha256: &str,
) -> Result<(), FcError> {
    let path = cache_file_path(cache_dir, relative_path)?;
    let metadata = fs::metadata(&path).map_err(|source| FcError::PathIo {
        path: path.clone(),
        source,
    })?;
    if !metadata.is_file() {
        return Err(invalid_cached_file(format!(
            "{label} is not a regular file: path={}",
            path.display()
        )));
    }
    let observed = sha256_file(&path)?;
    if observed != expected_sha256 {
        return Err(invalid_cached_file(format!(
            "{label} sha256 mismatch: path={} expected_sha256={expected_sha256} observed_sha256={observed}",
            path.display()
        )));
    }
    Ok(())
}

fn cache_file_path(cache_dir: &Path, relative_path: &str) -> Result<PathBuf, FcError> {
    if relative_path.is_empty()
        || relative_path.contains('/')
        || relative_path.contains('\\')
        || relative_path == "."
        || relative_path == ".."
    {
        return Err(invalid_cached_file(format!(
            "proof-cache path must be a file name inside the cache directory: {relative_path:?}"
        )));
    }
    Ok(cache_dir.join(relative_path))
}

fn verify_mode(path: &Path, expected: u32, field: &'static str) -> Result<(), FcError> {
    let observed = fs::metadata(path)
        .map_err(|source| FcError::PathIo {
            path: path.to_path_buf(),
            source,
        })?
        .permissions()
        .mode()
        & 0o777;
    if observed == expected {
        Ok(())
    } else {
        Err(FcError::Config(ConfigError::InvalidValue {
            field,
            reason: format!(
                "proof-cache mode mismatch: path={} expected={expected:o} observed={observed:o}",
                path.display()
            ),
        }))
    }
}

fn sha256_file(path: &Path) -> Result<String, FcError> {
    let bytes = fs::read(path).map_err(|source| FcError::PathIo {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(sha256_bytes(&bytes))
}

fn sha256_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn require_nonempty(label: &str, value: &str) -> Result<(), FcError> {
    if value.is_empty() {
        Err(invalid_manifest(format!("{label} must not be empty")))
    } else {
        Ok(())
    }
}

fn require_sha256(label: &str, value: &str) -> Result<(), FcError> {
    if value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(invalid_manifest(format!(
            "{label} must be a 64-hex sha256 digest"
        )))
    }
}

fn invalid_manifest(reason: String) -> FcError {
    FcError::Config(ConfigError::InvalidValue {
        field: "proof-cache.manifest",
        reason,
    })
}

fn invalid_cached_file(reason: String) -> FcError {
    FcError::Config(ConfigError::InvalidValue {
        field: "proof-cache.cached-file",
        reason,
    })
}

fn maybe_inject_write_failure() -> Result<(), FcError> {
    #[cfg(debug_assertions)]
    {
        if std::env::var_os("M80_INSTALL_INJECT_PROOF_CACHE_WRITE_FAILURE").is_some() {
            return Err(FcError::Config(ConfigError::InvalidValue {
                field: "proof-cache.write",
                reason: "injected proof-cache write failure".to_owned(),
            }));
        }
    }
    Ok(())
}

fn inject_manifest_digest_failure() -> bool {
    #[cfg(debug_assertions)]
    {
        std::env::var_os("M80_INSTALL_INJECT_PROOF_CACHE_DIGEST_FAILURE").is_some()
    }
    #[cfg(not(debug_assertions))]
    {
        false
    }
}

fn maybe_inject_mode_failure(paths: &[PathBuf]) -> Result<(), FcError> {
    #[cfg(debug_assertions)]
    {
        if std::env::var_os("M80_INSTALL_INJECT_PROOF_CACHE_MODE_FAILURE").is_some() {
            let Some(path) = paths.first() else {
                return Ok(());
            };
            fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|source| {
                FcError::PathIo {
                    path: path.clone(),
                    source,
                }
            })?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
