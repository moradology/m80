use std::fs;
use std::path::Path;

use serde::Serialize;

use super::sha256_bytes;

pub(super) fn write_proof_cache(cache_dir: &Path, tag: &str) {
    fs::create_dir_all(cache_dir).expect("create proof-cache dir");
    let integrity = proof_file(cache_dir, "m80-release-integrity.json", b"integrity\n");
    let attestation = proof_file(
        cache_dir,
        "m80-release-integrity.attestation.jsonl",
        b"attestation\n",
    );
    let metadata = proof_file(cache_dir, "m80-release-attestation.json", b"metadata\n");
    let asset_index = proof_file(cache_dir, "m80-release-assets.json", b"asset-index\n");
    let public_sha256s = proof_file(cache_dir, "SHA256SUMS", b"sha256s\n");
    let sidecar = proof_file(cache_dir, "m80-linux-x86_64.tar.gz.sha256", b"abc bundle\n");
    let trust = proof_file(cache_dir, "m80-release-trust-policy.json", b"trust\n");

    let payload = TestProofPayload {
        release_tag: tag.to_owned(),
        repository: "moradology/m80".to_owned(),
        target: "linux-x86_64".to_owned(),
        integrity_predicate: integrity,
        attestation_bundle: attestation,
        attestation_metadata: TestAttestationMetadataRef {
            file: metadata,
            signer_identity:
                "https://github.com/moradology/m80/.github/workflows/release.yml@refs/tags/v1.2.3"
                    .to_owned(),
            issuer: "https://token.actions.githubusercontent.com".to_owned(),
            keyset_id: "keyset".to_owned(),
            predicate_sha256: "c".repeat(64),
        },
        asset_index,
        public_sha256s,
        checksum_sidecars: vec![TestChecksumSidecarRef {
            path: sidecar.path,
            sha256: sidecar.sha256,
            subject: "bundle".to_owned(),
        }],
        trust_policy: TestTrustPolicyRef {
            path: trust.path,
            identity: "repository=moradology/m80".to_owned(),
            sha256: trust.sha256,
        },
        verifier_versions: TestVerifierVersions {
            m80_version: tag.to_owned(),
            gh_version: "gh version 2.0.0".to_owned(),
            release_integrity_schema_version: 1,
            asset_index_schema_version: 1,
        },
    };
    let manifest = TestProofManifest {
        schema_version: 1,
        manifest_digest: sha256_json(&payload),
        payload,
    };
    fs::write(
        cache_dir.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest).expect("encode proof-cache manifest"),
    )
    .expect("write proof-cache manifest");
}

fn proof_file(cache_dir: &Path, name: &str, bytes: &[u8]) -> TestProofFile {
    let path = cache_dir.join(name);
    fs::write(&path, bytes).expect("write proof-cache file");
    TestProofFile {
        path: name.to_owned(),
        sha256: sha256_bytes(bytes),
        size_bytes: bytes.len() as u64,
    }
}

fn sha256_json(value: &impl Serialize) -> String {
    sha256_bytes(&serde_json::to_vec(value).expect("encode proof-cache digest payload"))
}

#[derive(Serialize)]
struct TestProofManifest {
    schema_version: u32,
    manifest_digest: String,
    payload: TestProofPayload,
}

#[derive(Serialize)]
struct TestProofPayload {
    release_tag: String,
    repository: String,
    target: String,
    integrity_predicate: TestProofFile,
    attestation_bundle: TestProofFile,
    attestation_metadata: TestAttestationMetadataRef,
    asset_index: TestProofFile,
    public_sha256s: TestProofFile,
    checksum_sidecars: Vec<TestChecksumSidecarRef>,
    trust_policy: TestTrustPolicyRef,
    verifier_versions: TestVerifierVersions,
}

#[derive(Serialize)]
struct TestProofFile {
    path: String,
    sha256: String,
    size_bytes: u64,
}

#[derive(Serialize)]
struct TestAttestationMetadataRef {
    file: TestProofFile,
    signer_identity: String,
    issuer: String,
    keyset_id: String,
    predicate_sha256: String,
}

#[derive(Serialize)]
struct TestChecksumSidecarRef {
    path: String,
    sha256: String,
    subject: String,
}

#[derive(Serialize)]
struct TestTrustPolicyRef {
    path: String,
    identity: String,
    sha256: String,
}

#[derive(Serialize)]
struct TestVerifierVersions {
    m80_version: String,
    gh_version: String,
    release_integrity_schema_version: u32,
    asset_index_schema_version: u32,
}
