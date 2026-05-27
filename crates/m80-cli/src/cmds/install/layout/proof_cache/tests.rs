use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use serde_json::{json, Value};

use super::*;

#[test]
fn write_verified_release_proof_cache_copies_manifest_and_mode_checks_material() {
    let _guard = super::super::INSTALL_PREFLIGHT_ENV_LOCK.lock().unwrap();
    let fixture = verified_bundle_fixture();
    let final_root = tempfile::tempdir().expect("create install root");
    let final_dir = final_root.path().join("versions/v0.0.0");
    fs::create_dir_all(final_dir.join("artifacts")).expect("create artifacts dir");

    let manifest_path = write_verified_release_proof_cache(&fixture.bundle, &final_dir, "v0.0.0")
        .expect("write verified proof cache");

    assert_eq!(
        manifest_path,
        final_dir
            .join("artifacts")
            .join(PROOF_CACHE_DIR)
            .join(PROOF_CACHE_MANIFEST)
    );
    let manifest = read_proof_cache_manifest(&manifest_path).expect("read proof manifest");
    assert_eq!(manifest.payload.release_tag, "v0.0.0");
    assert_eq!(manifest.payload.repository, "moradology/m80");
    assert_eq!(manifest.payload.target, "linux-x86_64");
    assert_eq!(
        manifest.payload.attestation_metadata.keyset_id,
        "github-actions-oidc:m80-release-v1"
    );
    assert_eq!(
        manifest.payload.verifier_versions.attestation_verifier,
        "m80 native release-attestation verifier v1"
    );
    assert_eq!(file_mode(&manifest_path), PROOF_CACHE_FILE_MODE);
    assert!(manifest_path
        .parent()
        .expect("manifest has parent")
        .join("m80-release-integrity.json")
        .is_file());
    assert!(manifest_path
        .parent()
        .expect("manifest has parent")
        .join(TRUST_POLICY_NAME)
        .is_file());
}

#[test]
fn write_verified_release_proof_cache_rejects_existing_cache_target_file() {
    let fixture = verified_bundle_fixture();
    let final_root = tempfile::tempdir().expect("create install root");
    let final_dir = final_root.path().join("versions/v0.0.0");
    let cache_target = final_dir.join("artifacts").join(PROOF_CACHE_DIR);
    fs::create_dir_all(cache_target.parent().expect("cache target has parent"))
        .expect("create artifacts dir");
    fs::write(&cache_target, b"not a directory\n").expect("write cache target file");

    let err = write_verified_release_proof_cache(&fixture.bundle, &final_dir, "v0.0.0")
        .expect_err("pre-existing cache target file must fail");

    assert!(
        matches!(err, FcError::PathIo { ref path, .. } if path == &cache_target),
        "unexpected error: {err:?}"
    );
    assert!(
        !cache_target.join(PROOF_CACHE_MANIFEST).exists(),
        "cache manifest must not be written under a non-directory target"
    );
    assert_eq!(
        fs::read(&cache_target).expect("read rejected cache target file"),
        b"not a directory\n",
        "cache target collision must not rewrite the existing file"
    );
}

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
        err.to_string().contains("asset_index.sha256") && err.to_string().contains("64-hex sha256"),
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
        err.to_string().contains("manifest_digest") && err.to_string().contains("64-hex sha256"),
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
            signer_identity: "moradology/m80/.github/workflows/release-artifacts.yml".to_owned(),
            issuer: "https://token.actions.githubusercontent.com".to_owned(),
            keyset_id: "github-actions-oidc:m80-release-v1".to_owned(),
            predicate_sha256: digest("1"),
        },
        asset_index: proof_file("m80-release-assets.json", "4", 500),
        trust_policy: TrustPolicyRef {
            path: "m80-release-trust-policy.json".to_owned(),
            identity: "github-actions-oidc:m80-release-v1".to_owned(),
            sha256: digest("8"),
        },
        verifier_versions: VerifierVersions {
            m80_version: "v0.0.0".to_owned(),
            attestation_verifier: "m80 native release-attestation verifier v1".to_owned(),
            release_integrity_schema_version: 1,
            asset_index_schema_version: 2,
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

struct VerifiedBundleFixture {
    _temp: tempfile::TempDir,
    bundle: VerifiedOfficialReleaseBundle,
}

fn verified_bundle_fixture() -> VerifiedBundleFixture {
    let temp = tempfile::tempdir().expect("create materials temp");
    let material_dir = temp.path().join("materials");
    fs::create_dir(&material_dir).expect("create material dir");
    let mut material_paths = BTreeMap::new();
    for (class, name, body) in [
        (
            "release-integrity-predicate",
            "m80-release-integrity.json",
            b"{\"schema_version\":1}\n".as_slice(),
        ),
        (
            "release-attestation-bundle",
            "m80-release-integrity.attestation.jsonl",
            b"{\"bundle\":\"fixture\"}\n".as_slice(),
        ),
        (
            "release-attestation-metadata",
            "m80-release-attestation.json",
            b"{\"schema_version\":1}\n".as_slice(),
        ),
        (
            "asset-index",
            "m80-release-assets.json",
            b"{\"schema_version\":1}\n".as_slice(),
        ),
    ] {
        let path = material_dir.join(name);
        fs::write(&path, body).expect("write material fixture");
        material_paths.insert(class, path);
    }

    let bundle_path = material_dir.join("m80-linux-x86_64.tar.gz");
    fs::write(&bundle_path, b"bundle\n").expect("write bundle fixture");
    material_paths.insert("bundle", bundle_path.clone());
    let bundle = VerifiedOfficialReleaseBundle {
        _temp_dir: tempfile::tempdir().expect("own verified temp"),
        bundle_path,
        material_paths,
        summary: super::super::release_material::ReleaseVerificationSummary {
            release_tag: "v0.0.0".to_owned(),
            repository: "moradology/m80".to_owned(),
            target: "linux-x86_64".to_owned(),
            bundle_asset: "m80-linux-x86_64.tar.gz".to_owned(),
            bundle_url:
                "https://github.com/moradology/m80/releases/download/v0.0.0/m80-linux-x86_64.tar.gz"
                    .to_owned(),
            bundle_sha256: "a".repeat(64),
            install_sh_sha256: "d".repeat(64),
            predicate_sha256: "1".repeat(64),
            asset_index_sha256: "3".repeat(64),
            attestation_signer: "moradology/m80/.github/workflows/release-artifacts.yml".to_owned(),
            attestation_issuer: "https://token.actions.githubusercontent.com".to_owned(),
            attestation_keyset_id: "github-actions-oidc:m80-release-v1".to_owned(),
            source_commit: "0123456789abcdef0123456789abcdef01234567".to_owned(),
            release_integrity_schema_version: 1,
        },
    };
    VerifiedBundleFixture {
        _temp: temp,
        bundle,
    }
}

fn file_mode(path: &Path) -> u32 {
    fs::metadata(path).expect("read mode").permissions().mode() & 0o777
}
