#![allow(dead_code)]

use std::fs;
use std::path::Path;

use m80_firecracker::{ConfigError, FcError};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub(super) const PROOF_CACHE_DIR: &str = "release-proof-cache";
pub(super) const PROOF_CACHE_MANIFEST: &str = "manifest.json";
pub(super) const PROOF_CACHE_SCHEMA_VERSION: u32 = 1;

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
    pub(super) gh_version: String,
    pub(super) release_integrity_schema_version: u32,
    pub(super) asset_index_schema_version: u32,
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
        "verifier_versions.gh_version",
        &payload.verifier_versions.gh_version,
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

#[cfg(test)]
mod tests {
    use serde_json::{json, Value};

    use super::*;

    #[test]
    fn complete_manifest_parses_and_validates_digest() {
        let manifest = valid_manifest();
        let fixture = write_manifest(&manifest);

        let parsed = read_proof_cache_manifest(&fixture.path)
            .expect("complete proof-cache manifest should parse");

        assert_eq!(parsed.schema_version, PROOF_CACHE_SCHEMA_VERSION);
        assert_eq!(parsed.payload.release_tag, "v0.0.0");
        assert_eq!(
            parsed.payload.attestation_metadata.keyset_id,
            "github-actions-oidc:m80-release-v1"
        );
        assert_eq!(parsed.payload.checksum_sidecars.len(), 2);
    }

    #[test]
    fn missing_required_field_fails_closed() {
        let mut value = valid_manifest_json();
        value["payload"]
            .as_object_mut()
            .expect("payload should be an object")
            .remove("integrity_predicate");
        let fixture = write_value(&value);

        let err = read_proof_cache_manifest(&fixture.path)
            .expect_err("missing integrity_predicate should fail");

        assert!(
            err.to_string()
                .contains("missing field `integrity_predicate`"),
            "{err}"
        );
    }

    #[test]
    fn unknown_field_fails_closed() {
        let mut value = valid_manifest_json();
        value["payload"]
            .as_object_mut()
            .expect("payload should be an object")
            .insert("surprise".to_owned(), json!(true));
        let fixture = write_value(&value);

        let err = read_proof_cache_manifest(&fixture.path).expect_err("unknown field should fail");

        assert!(
            err.to_string().contains("unknown field `surprise`"),
            "{err}"
        );
    }

    #[test]
    fn malformed_material_digest_fails_closed() {
        let mut manifest = valid_manifest();
        manifest.payload.asset_index.sha256 = "not-a-digest".to_owned();
        manifest.manifest_digest =
            proof_cache_manifest_digest(&manifest.payload).expect("digest test payload");
        let fixture = write_manifest(&manifest);

        let err = read_proof_cache_manifest(&fixture.path)
            .expect_err("malformed material digest should fail");

        assert!(
            err.to_string().contains("asset_index.sha256")
                && err.to_string().contains("64-hex sha256"),
            "{err}"
        );
    }

    #[test]
    fn malformed_manifest_digest_fails_closed() {
        let mut manifest = valid_manifest();
        manifest.manifest_digest = "not-a-digest".to_owned();
        let fixture = write_manifest(&manifest);

        let err = read_proof_cache_manifest(&fixture.path)
            .expect_err("malformed manifest digest should fail");

        assert!(
            err.to_string().contains("manifest_digest")
                && err.to_string().contains("64-hex sha256"),
            "{err}"
        );
    }

    fn valid_manifest_json() -> Value {
        serde_json::to_value(valid_manifest()).expect("serialize test manifest")
    }

    fn valid_manifest() -> ProofCacheManifest {
        let payload = ProofCachePayload {
            release_tag: "v0.0.0".to_owned(),
            repository: "moradology/m80".to_owned(),
            target: "linux-x86_64".to_owned(),
            integrity_predicate: proof_file("m80-release-integrity.json", "1", 1200),
            attestation_bundle: proof_file("m80-release-integrity.attestation.jsonl", "2", 900),
            attestation_metadata: AttestationMetadataRef {
                file: proof_file("m80-release-attestation.json", "3", 700),
                signer_identity: "moradology/m80/.github/workflows/release-artifacts.yml"
                    .to_owned(),
                issuer: "https://token.actions.githubusercontent.com".to_owned(),
                keyset_id: "github-actions-oidc:m80-release-v1".to_owned(),
                predicate_sha256: digest("1"),
            },
            asset_index: proof_file("m80-release-assets.json", "4", 500),
            public_sha256s: proof_file("SHA256SUMS", "5", 400),
            checksum_sidecars: vec![
                ChecksumSidecarRef {
                    path: "m80-linux-x86_64.tar.gz.sha256".to_owned(),
                    sha256: digest("6"),
                    subject: "m80-linux-x86_64.tar.gz".to_owned(),
                },
                ChecksumSidecarRef {
                    path: "install.sh.sha256".to_owned(),
                    sha256: digest("7"),
                    subject: "install.sh".to_owned(),
                },
            ],
            trust_policy: TrustPolicyRef {
                path: "m80-release-trust-policy.json".to_owned(),
                identity: "github-actions-oidc:m80-release-v1".to_owned(),
                sha256: digest("8"),
            },
            verifier_versions: VerifierVersions {
                m80_version: "v0.0.0".to_owned(),
                gh_version: "gh version 2.75.0".to_owned(),
                release_integrity_schema_version: 1,
                asset_index_schema_version: 1,
            },
        };
        let manifest_digest = proof_cache_manifest_digest(&payload).expect("digest test payload");
        ProofCacheManifest {
            schema_version: PROOF_CACHE_SCHEMA_VERSION,
            manifest_digest,
            payload,
        }
    }

    fn proof_file(path: &str, seed: &str, size_bytes: u64) -> ProofCacheFile {
        ProofCacheFile {
            path: path.to_owned(),
            sha256: digest(seed),
            size_bytes,
        }
    }

    fn digest(seed: &str) -> String {
        seed.repeat(64 / seed.len())
    }

    struct ManifestFixture {
        _temp: tempfile::TempDir,
        path: std::path::PathBuf,
    }

    fn write_manifest(manifest: &ProofCacheManifest) -> ManifestFixture {
        write_value(&serde_json::to_value(manifest).expect("serialize test manifest"))
    }

    fn write_value(value: &Value) -> ManifestFixture {
        let temp = tempfile::tempdir().expect("create temp proof-cache manifest dir");
        let path = temp.path().join(PROOF_CACHE_MANIFEST);
        std::fs::write(
            &path,
            serde_json::to_vec_pretty(value).expect("serialize test manifest JSON"),
        )
        .expect("write test manifest");
        ManifestFixture { _temp: temp, path }
    }
}
