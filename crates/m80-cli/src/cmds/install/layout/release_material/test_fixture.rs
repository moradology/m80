use std::fs;
use std::path::Path;

use base64::Engine;
use m80_firecracker::FcError;
use sha2::{Digest, Sha256};

use super::test_env::{fake_gh_fixture, write_fake_curl, EnvVarGuard};

#[derive(Clone, Copy, Default)]
pub(super) struct ReleaseFixtureOptions {
    pub(super) release_tag: Option<&'static str>,
    pub(super) omit: Option<&'static str>,
    pub(super) tamper_bundle: bool,
    pub(super) wrong_bundle_checksum: bool,
    pub(super) stale_asset_index: bool,
    pub(super) stale_bundle_size: bool,
    pub(super) mismatched_attestation: bool,
    pub(super) wrong_attestation_signer: bool,
    pub(super) wrong_attestation_issuer: bool,
    pub(super) wrong_attestation_keyset: bool,
    pub(super) wrong_commit_sha: bool,
    pub(super) wrong_repository: bool,
    pub(super) wrong_release_tag: bool,
    pub(super) missing_install_digest: bool,
    pub(super) stale_public_sha256s: bool,
    pub(super) bad_predicate_subject: bool,
    pub(super) gh_failure: bool,
    pub(super) gh_omit_subject: bool,
    pub(super) gh_wrong_subject_digest: bool,
    pub(super) gh_wrong_source_ref: bool,
    pub(super) gh_wrong_commit: bool,
    pub(super) self_hosted_runner: bool,
    pub(super) alternate_install_script: bool,
}

pub(super) struct ReleaseFixture {
    pub(super) bundle_url: String,
    pub(super) bundle_sha256: String,
    pub(super) asset_index_sha256: String,
    pub(super) install_sha256: String,
    pub(super) predicate_sha256: String,
}

pub(super) fn verifier_error(options: ReleaseFixtureOptions) -> FcError {
    verifier_error_with_curl_log(options).0
}

pub(super) fn verifier_error_with_curl_log(options: ReleaseFixtureOptions) -> (FcError, String) {
    let _guard = super::super::INSTALL_PREFLIGHT_ENV_LOCK.lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let material_dir = temp.path().join("materials");
    fs::create_dir(&material_dir).unwrap();
    let log_path = temp.path().join("curl.log");
    let bin_dir = temp.path().join("bin");
    fs::create_dir(&bin_dir).unwrap();
    write_fake_curl(&bin_dir);
    let fixture = write_direct_release_materials_with(&material_dir, options);

    let _path_env = EnvVarGuard::prepend_path(&bin_dir);
    let _material_env = EnvVarGuard::set("M80_FAKE_CURL_MATERIAL_DIR", &material_dir);
    let _log_env = EnvVarGuard::set("M80_FAKE_CURL_LOG", &log_path);
    let _gh_env = EnvVarGuard::set(
        "M80_RELEASE_ATTESTATION_GH",
        &fake_gh_fixture("fake-gh-attestation-supported.sh"),
    );
    let _expect_source_digest = EnvVarGuard::set_value(
        "M80_FAKE_GH_EXPECT_SOURCE_DIGEST",
        "0123456789abcdef0123456789abcdef01234567",
    );
    let _gh_failure = options
        .gh_failure
        .then(|| EnvVarGuard::set_value("M80_FAKE_GH_FAIL", "1"));
    let _gh_omit_subject = options
        .gh_omit_subject
        .then(|| EnvVarGuard::set_value("M80_FAKE_GH_OMIT_SUBJECT", "1"));
    let _gh_wrong_subject_digest = options
        .gh_wrong_subject_digest
        .then(|| EnvVarGuard::set_value("M80_FAKE_GH_WRONG_SUBJECT_DIGEST", "1"));
    let _gh_wrong_source_ref = options
        .gh_wrong_source_ref
        .then(|| EnvVarGuard::set_value("M80_FAKE_GH_EXPECT_SOURCE_REF", "refs/tags/v9.9.9"));

    let err = super::verify_official_release_bundle(&fixture.bundle_url).unwrap_err();
    let log = fs::read_to_string(&log_path).unwrap_or_default();
    (err, log)
}

pub(super) fn write_direct_release_materials_with(
    material_dir: &Path,
    options: ReleaseFixtureOptions,
) -> ReleaseFixture {
    write_direct_release_materials_with_bundle_bytes(
        material_dir,
        options,
        b"release bundle bytes\n",
    )
}

pub(super) fn write_direct_release_materials_with_bundle_bytes(
    material_dir: &Path,
    options: ReleaseFixtureOptions,
    bundle_bytes: &[u8],
) -> ReleaseFixture {
    let release_tag = options.release_tag.unwrap_or("v0.0.0");
    let bundle_name = "m80-linux-x86_64.tar.gz";
    let bundle_url = crate::release_urls::release_asset_url(release_tag, bundle_name);
    let tampered_bundle_bytes = b"tampered release bundle bytes\n";
    let bundle_sha256 = sha256_bytes(bundle_bytes);
    let bundle_size = bundle_bytes.len();
    let row_bundle_sha256 = if options.stale_asset_index {
        "d".repeat(64)
    } else {
        bundle_sha256.clone()
    };
    let row_bundle_size = if options.stale_asset_index || options.stale_bundle_size {
        bundle_size as u64 + 1
    } else {
        bundle_size as u64
    };
    let written_bundle = if options.tamper_bundle {
        tampered_bundle_bytes.as_slice()
    } else {
        bundle_bytes
    };
    write_material(material_dir, bundle_name, written_bundle, options.omit);

    let metadata_name = "m80-linux-x86_64.bundle.json";
    let metadata_bytes = serde_json::to_vec_pretty(&serde_json::json!({
        "release_tag": release_tag,
        "m80_version": release_tag,
        "package_version": "0.0.0",
        "target": "linux-x86_64",
        "os": "linux",
        "arch": "x86_64",
        "image_kind": "minimal",
        "guest_protocol_version": 1,
        "manifest_schema_version": 1,
        "expected_firecracker_version": "v1.15.1"
    }))
    .unwrap();
    let metadata_sha256 = sha256_bytes(&metadata_bytes);
    write_material(material_dir, metadata_name, &metadata_bytes, options.omit);

    let index = serde_json::json!({
        "schema_version": 1,
        "release_tag": release_tag,
        "assets": [{
            "name": bundle_name,
            "url": bundle_url.as_str(),
            "sha256": row_bundle_sha256.as_str(),
            "size_bytes": row_bundle_size,
            "metadata_name": metadata_name,
            "metadata_sha256": metadata_sha256.as_str(),
            "checksum_name": format!("{bundle_name}.sha256"),
            "signature_name": null,
            "attestation_name": "m80-release-integrity.attestation.jsonl",
            "target": "linux-x86_64",
            "os": "linux",
            "arch": "x86_64",
            "image_kind": "minimal",
            "release_tag": release_tag,
            "m80_version": release_tag,
            "guest_protocol_version": 1,
            "manifest_schema_version": 1,
            "expected_firecracker_version": "v1.15.1"
        }]
    });
    let mut index_bytes = serde_json::to_vec_pretty(&index).unwrap();
    index_bytes.push(b'\n');
    let index_sha256 = sha256_bytes(&index_bytes);

    let install_bytes: &[u8] = if options.alternate_install_script {
        b"#!/bin/sh\nprintf 'alternate install script\\n'\nexit 0\n"
    } else {
        b"#!/bin/sh\nexit 0\n"
    };
    let selector_bytes = format!(
        "schema_version\t1\nrelease_tag\t{release_tag}\ncolumns\tos\tarch\timage_kind\tbundle_name\tbundle_url\tbundle_sha256\tsize_bytes\tmetadata_name\tmetadata_sha256\tchecksum_name\tsignature_name\tattestation_name\tm80_version\nrow\tlinux\tx86_64\tminimal\t{bundle_name}\t{bundle_url}\t{bundle_sha256}\t{bundle_size}\t{metadata_name}\t{metadata_sha256}\t{bundle_name}.sha256\t-\tm80-release-integrity.attestation.jsonl\t{release_tag}\n"
    );
    let commit_sha = if options.wrong_commit_sha {
        "1111111111111111111111111111111111111111"
    } else {
        "0123456789abcdef0123456789abcdef01234567"
    };
    let repository = if options.wrong_repository {
        "moradology/not-m80"
    } else {
        "moradology/m80"
    };
    let predicate_release_tag = if options.wrong_release_tag {
        "v9.9.9"
    } else {
        release_tag
    };
    let build_bytes = serde_json::to_vec_pretty(&serde_json::json!({
        "schema_version": 1,
        "release_tag": release_tag,
        "source_commit": commit_sha
    }))
    .unwrap();

    let install_sha256 = sha256_bytes(install_bytes);
    let selector_sha256 = sha256_bytes(selector_bytes.as_bytes());
    let build_sha256 = sha256_bytes(&build_bytes);
    let metadata_sidecar_name = format!("{metadata_name}.sha256");
    let bundle_checksum_name = format!("{bundle_name}.sha256");
    let bundle_checksum_sha = if options.wrong_bundle_checksum {
        "e".repeat(64)
    } else {
        bundle_sha256.clone()
    };
    let bundle_checksum_bytes = format!("{bundle_checksum_sha}  {bundle_name}\n");
    let metadata_checksum_bytes = format!("{metadata_sha256}  {metadata_name}\n");
    let index_checksum_bytes = format!("{index_sha256}  m80-release-assets.json\n");
    let install_checksum_bytes = format!("{install_sha256}  install.sh\n");
    let selector_checksum_bytes = format!("{selector_sha256}  m80-bootstrap-selector.tsv\n");
    let build_checksum_bytes = format!("{build_sha256}  m80-release-build.json\n");

    write_material(
        material_dir,
        &bundle_checksum_name,
        bundle_checksum_bytes.as_bytes(),
        options.omit,
    );
    write_material(
        material_dir,
        &metadata_sidecar_name,
        metadata_checksum_bytes.as_bytes(),
        options.omit,
    );
    write_material(
        material_dir,
        "m80-release-assets.json",
        &index_bytes,
        options.omit,
    );
    write_material(
        material_dir,
        "m80-release-assets.json.sha256",
        index_checksum_bytes.as_bytes(),
        options.omit,
    );
    write_material(material_dir, "install.sh", install_bytes, options.omit);
    write_material(
        material_dir,
        "install.sh.sha256",
        install_checksum_bytes.as_bytes(),
        options.omit,
    );
    write_material(
        material_dir,
        "m80-bootstrap-selector.tsv",
        selector_bytes.as_bytes(),
        options.omit,
    );
    write_material(
        material_dir,
        "m80-bootstrap-selector.tsv.sha256",
        selector_checksum_bytes.as_bytes(),
        options.omit,
    );
    write_material(
        material_dir,
        "m80-release-build.json",
        &build_bytes,
        options.omit,
    );
    write_material(
        material_dir,
        "m80-release-build.json.sha256",
        build_checksum_bytes.as_bytes(),
        options.omit,
    );

    let public_rows = vec![
        (bundle_name.to_owned(), bundle_sha256.clone()),
        (
            bundle_checksum_name.clone(),
            sha256_bytes(bundle_checksum_bytes.as_bytes()),
        ),
        (metadata_name.to_owned(), metadata_sha256.clone()),
        (
            metadata_sidecar_name.clone(),
            sha256_bytes(metadata_checksum_bytes.as_bytes()),
        ),
        ("m80-release-assets.json".to_owned(), index_sha256.clone()),
        (
            "m80-release-assets.json.sha256".to_owned(),
            sha256_bytes(index_checksum_bytes.as_bytes()),
        ),
        ("install.sh".to_owned(), install_sha256.clone()),
        (
            "install.sh.sha256".to_owned(),
            sha256_bytes(install_checksum_bytes.as_bytes()),
        ),
        (
            "m80-bootstrap-selector.tsv".to_owned(),
            selector_sha256.clone(),
        ),
        (
            "m80-bootstrap-selector.tsv.sha256".to_owned(),
            sha256_bytes(selector_checksum_bytes.as_bytes()),
        ),
        ("m80-release-build.json".to_owned(), build_sha256.clone()),
        (
            "m80-release-build.json.sha256".to_owned(),
            sha256_bytes(build_checksum_bytes.as_bytes()),
        ),
    ];
    let mut public_sha256s = String::new();
    for (name, digest) in public_rows {
        if options.missing_install_digest && name == "install.sh" {
            continue;
        }
        let digest = if options.stale_public_sha256s && name == "install.sh" {
            "f".repeat(64)
        } else {
            digest
        };
        public_sha256s.push_str(&format!("{digest}  {name}\n"));
    }
    let public_sha256s_sha = sha256_bytes(public_sha256s.as_bytes());
    write_material(
        material_dir,
        "SHA256SUMS",
        public_sha256s.as_bytes(),
        options.omit,
    );

    let install_subject_sha256 = if options.bad_predicate_subject {
        "e".repeat(64)
    } else {
        install_sha256.clone()
    };
    let subjects = [
        subject(
            bundle_name,
            "release-bundle",
            &bundle_sha256,
            bundle_bytes.len(),
        ),
        subject(
            &bundle_checksum_name,
            "checksum-sidecar",
            &sha256_bytes(bundle_checksum_bytes.as_bytes()),
            bundle_checksum_bytes.len(),
        ),
        subject(
            metadata_name,
            "bundle-metadata",
            &metadata_sha256,
            metadata_bytes.len(),
        ),
        subject(
            &metadata_sidecar_name,
            "checksum-sidecar",
            &sha256_bytes(metadata_checksum_bytes.as_bytes()),
            metadata_checksum_bytes.len(),
        ),
        subject(
            "m80-release-assets.json",
            "asset-index",
            &index_sha256,
            index_bytes.len(),
        ),
        subject(
            "m80-release-assets.json.sha256",
            "checksum-sidecar",
            &sha256_bytes(index_checksum_bytes.as_bytes()),
            index_checksum_bytes.len(),
        ),
        subject(
            "install.sh",
            "installer",
            &install_subject_sha256,
            install_bytes.len(),
        ),
        subject(
            "install.sh.sha256",
            "checksum-sidecar",
            &sha256_bytes(install_checksum_bytes.as_bytes()),
            install_checksum_bytes.len(),
        ),
        subject(
            "m80-bootstrap-selector.tsv",
            "bootstrap-selector",
            &selector_sha256,
            selector_bytes.len(),
        ),
        subject(
            "m80-bootstrap-selector.tsv.sha256",
            "checksum-sidecar",
            &sha256_bytes(selector_checksum_bytes.as_bytes()),
            selector_checksum_bytes.len(),
        ),
        subject(
            "m80-release-build.json",
            "build-manifest",
            &build_sha256,
            build_bytes.len(),
        ),
        subject(
            "m80-release-build.json.sha256",
            "checksum-sidecar",
            &sha256_bytes(build_checksum_bytes.as_bytes()),
            build_checksum_bytes.len(),
        ),
        subject(
            "SHA256SUMS",
            "checksum-manifest",
            &public_sha256s_sha,
            public_sha256s.len(),
        ),
    ];
    let integrity = serde_json::json!({
        "schema_version": 1,
        "mechanism": "github-artifact-attestation",
        "repository": repository,
        "release_tag": predicate_release_tag,
        "commit_sha": commit_sha,
        "target": "linux-x86_64",
        "rust_toolchain": "rustc 1.82.0",
        "m80_package_version": "0.0.0",
        "bundle_metadata_name": metadata_name,
        "bundle_metadata_sha256": metadata_sha256,
        "subjects": subjects
    });
    let mut integrity_bytes = serde_json::to_vec_pretty(&integrity).unwrap();
    integrity_bytes.push(b'\n');
    let predicate_sha256 = sha256_bytes(&integrity_bytes);
    write_material(
        material_dir,
        "m80-release-integrity.json",
        &integrity_bytes,
        options.omit,
    );
    write_material(
        material_dir,
        "m80-release-integrity.attestation.jsonl",
        attestation_bundle_bytes(
            &predicate_release_tag,
            commit_sha,
            &predicate_sha256,
            options,
        )
        .as_bytes(),
        options.omit,
    );
    let attestation_predicate_sha = if options.mismatched_attestation {
        "f".repeat(64)
    } else {
        predicate_sha256.clone()
    };
    let signer_identity = if options.wrong_attestation_signer {
        "moradology/m80/.github/workflows/other.yml"
    } else {
        "moradology/m80/.github/workflows/release-artifacts.yml"
    };
    let issuer = if options.wrong_attestation_issuer {
        "https://example.invalid/token"
    } else {
        "https://token.actions.githubusercontent.com"
    };
    let keyset_id = if options.wrong_attestation_keyset {
        "github-actions-oidc:old"
    } else {
        "github-actions-oidc:m80-release-v1"
    };
    let attestation = serde_json::json!({
        "schema_version": 1,
        "mechanism": "github-artifact-attestation",
        "repository": repository,
        "release_tag": predicate_release_tag,
        "predicate_sha256": attestation_predicate_sha,
        "signer_identity": signer_identity,
        "issuer": issuer,
        "keyset_id": keyset_id,
        "certificate_not_before": "2026-01-01T00:00:00Z",
        "certificate_not_after": "2027-01-01T00:00:00Z"
    });
    let mut attestation_bytes = serde_json::to_vec_pretty(&attestation).unwrap();
    attestation_bytes.push(b'\n');
    write_material(
        material_dir,
        "m80-release-attestation.json",
        &attestation_bytes,
        options.omit,
    );

    ReleaseFixture {
        bundle_url,
        bundle_sha256,
        asset_index_sha256: index_sha256,
        install_sha256,
        predicate_sha256,
    }
}

fn subject(name: &str, kind: &str, sha256: &str, size_bytes: usize) -> serde_json::Value {
    serde_json::json!({
        "name": name,
        "kind": kind,
        "sha256": sha256,
        "size_bytes": size_bytes
    })
}

fn write_material(material_dir: &Path, name: &str, bytes: &[u8], omit: Option<&str>) {
    if omit == Some(name) {
        return;
    }
    fs::write(material_dir.join(name), bytes).unwrap();
}

fn sha256_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn attestation_bundle_bytes(
    release_tag: &str,
    commit_sha: &str,
    predicate_sha256: &str,
    options: ReleaseFixtureOptions,
) -> String {
    let workflow_ref = if options.gh_wrong_source_ref {
        "refs/tags/v9.9.9"
    } else {
        return attestation_bundle_bytes_with_workflow_ref(
            release_tag,
            commit_sha,
            predicate_sha256,
            options,
            &format!("refs/tags/{release_tag}"),
        );
    };
    attestation_bundle_bytes_with_workflow_ref(
        release_tag,
        commit_sha,
        predicate_sha256,
        options,
        workflow_ref,
    )
}

fn attestation_bundle_bytes_with_workflow_ref(
    release_tag: &str,
    commit_sha: &str,
    predicate_sha256: &str,
    options: ReleaseFixtureOptions,
    workflow_ref: &str,
) -> String {
    let bundle_commit_sha = if options.gh_wrong_commit {
        "1111111111111111111111111111111111111111"
    } else {
        commit_sha
    };
    let subject_sha = if options.gh_wrong_subject_digest {
        "0".repeat(64)
    } else {
        predicate_sha256.to_owned()
    };
    let subject_name = if options.gh_omit_subject {
        serde_json::json!([])
    } else {
        serde_json::json!([{
            "name": "m80-release-integrity.json",
            "digest": {"sha256": subject_sha}
        }])
    };
    let runner_environment = if options.self_hosted_runner {
        "self-hosted"
    } else {
        "github-hosted"
    };
    let statement = serde_json::json!({
        "_type": "https://in-toto.io/Statement/v1",
        "subject": subject_name,
        "predicateType": "https://slsa.dev/provenance/v1",
        "predicate": {
            "buildDefinition": {
                "buildType": "https://actions.github.io/buildtypes/workflow/v1",
                "externalParameters": {
                    "workflow": {
                        "ref": workflow_ref,
                        "repository": "https://github.com/moradology/m80",
                        "path": ".github/workflows/release-artifacts.yml"
                    }
                },
                "internalParameters": {
                    "github": {
                        "event_name": "push",
                        "runner_environment": runner_environment
                    }
                },
                "resolvedDependencies": [{
                    "uri": format!("git+https://github.com/moradology/m80@refs/tags/{release_tag}"),
                    "digest": {"gitCommit": bundle_commit_sha}
                }]
            },
            "runDetails": {
                "builder": {
                    "id": format!("https://github.com/moradology/m80/.github/workflows/release-artifacts.yml@refs/tags/{release_tag}")
                },
                "metadata": {
                    "invocationId": "https://github.com/moradology/m80/actions/runs/1/attempts/1"
                }
            }
        }
    });
    let payload =
        base64::engine::general_purpose::STANDARD.encode(serde_json::to_vec(&statement).unwrap());
    serde_json::to_string_pretty(&serde_json::json!({
        "mediaType": if options.gh_failure {
            "application/vnd.dev.sigstore.bundle.invalid"
        } else {
            "application/vnd.dev.sigstore.bundle.v0.3+json"
        },
        "verificationMaterial": {
            "certificate": {"rawBytes": "fixture-certificate"},
            "tlogEntries": [{"logIndex": "1"}]
        },
        "dsseEnvelope": {
            "payloadType": "application/vnd.in-toto+json",
            "payload": payload,
            "signatures": [{"sig": "fixture-signature"}]
        }
    }))
    .unwrap()
        + "\n"
}
