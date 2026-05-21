use std::fs;
use std::path::Path;

use m80_firecracker::FcError;
use sha2::{Digest, Sha256};

use super::test_env::{write_fake_curl, EnvVarGuard};

#[derive(Default)]
pub(super) struct ReleaseFixtureOptions {
    pub(super) omit: Option<&'static str>,
    pub(super) tamper_bundle: bool,
    pub(super) wrong_bundle_checksum: bool,
    pub(super) stale_asset_index: bool,
    pub(super) mismatched_attestation: bool,
    pub(super) missing_install_digest: bool,
}

pub(super) struct ReleaseFixture {
    pub(super) bundle_url: String,
    pub(super) bundle_sha256: String,
    pub(super) install_sha256: String,
    pub(super) predicate_sha256: String,
}

pub(super) fn verifier_error(options: ReleaseFixtureOptions) -> FcError {
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

    super::verify_official_release_bundle(&fixture.bundle_url).unwrap_err()
}

pub(super) fn write_direct_release_materials_with(
    material_dir: &Path,
    options: ReleaseFixtureOptions,
) -> ReleaseFixture {
    let bundle_name = "m80-linux-x86_64.tar.gz";
    let bundle_url = crate::release_urls::release_asset_url("v0.0.0", bundle_name);
    let bundle_bytes = b"release bundle bytes\n";
    let tampered_bundle_bytes = b"tampered release bundle bytes\n";
    let bundle_sha256 = sha256_bytes(bundle_bytes);
    let bundle_size = bundle_bytes.len();
    let row_bundle_sha256 = if options.stale_asset_index {
        "d".repeat(64)
    } else {
        bundle_sha256.clone()
    };
    let row_bundle_size = if options.stale_asset_index {
        bundle_size as u64 + 1
    } else {
        bundle_size as u64
    };
    let written_bundle = if options.tamper_bundle {
        tampered_bundle_bytes.as_slice()
    } else {
        bundle_bytes.as_slice()
    };
    write_material(material_dir, bundle_name, written_bundle, options.omit);

    let metadata_name = "m80-linux-x86_64.bundle.json";
    let metadata_bytes = serde_json::to_vec_pretty(&serde_json::json!({
        "release_tag": "v0.0.0",
        "m80_version": "v0.0.0",
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
        "release_tag": "v0.0.0",
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
            "release_tag": "v0.0.0",
            "m80_version": "v0.0.0",
            "guest_protocol_version": 1,
            "manifest_schema_version": 1,
            "expected_firecracker_version": "v1.15.1"
        }]
    });
    let mut index_bytes = serde_json::to_vec_pretty(&index).unwrap();
    index_bytes.push(b'\n');
    let index_sha256 = sha256_bytes(&index_bytes);

    let install_bytes = b"#!/bin/sh\nexit 0\n";
    let selector_bytes = format!(
        "schema_version\t1\nrelease_tag\tv0.0.0\ncolumns\tos\tarch\timage_kind\tbundle_name\tbundle_url\tbundle_sha256\tsize_bytes\tmetadata_name\tmetadata_sha256\tchecksum_name\tsignature_name\tattestation_name\tm80_version\nrow\tlinux\tx86_64\tminimal\t{bundle_name}\t{bundle_url}\t{bundle_sha256}\t{bundle_size}\t{metadata_name}\t{metadata_sha256}\t{bundle_name}.sha256\t-\tm80-release-integrity.attestation.jsonl\tv0.0.0\n"
    );
    let build_bytes = serde_json::to_vec_pretty(&serde_json::json!({
        "schema_version": 1,
        "release_tag": "v0.0.0",
        "source_commit": "0123456789abcdef0123456789abcdef01234567"
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
        public_sha256s.push_str(&format!("{digest}  {name}\n"));
    }
    let public_sha256s_sha = sha256_bytes(public_sha256s.as_bytes());
    write_material(
        material_dir,
        "SHA256SUMS",
        public_sha256s.as_bytes(),
        options.omit,
    );

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
            &install_sha256,
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
        "repository": "moradology/m80",
        "release_tag": "v0.0.0",
        "commit_sha": "0123456789abcdef0123456789abcdef01234567",
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
        b"{\"bundle\":\"fixture\"}\n",
        options.omit,
    );
    let attestation_predicate_sha = if options.mismatched_attestation {
        "f".repeat(64)
    } else {
        predicate_sha256.clone()
    };
    let attestation = serde_json::json!({
        "schema_version": 1,
        "mechanism": "github-artifact-attestation",
        "repository": "moradology/m80",
        "release_tag": "v0.0.0",
        "predicate_sha256": attestation_predicate_sha,
        "signer_identity": "moradology/m80/.github/workflows/release-artifacts.yml",
        "issuer": "https://token.actions.githubusercontent.com",
        "keyset_id": "fixture-keyset",
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
