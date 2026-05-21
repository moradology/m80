use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use super::super::super::{InstallPlan, SourceKind, SourcePlan};
use super::*;

#[test]
fn direct_plan_lists_same_tag_urls_and_expected_identity_before_fetch() {
    let plan = ReleaseMaterialPlan::from_index_material(valid_material()).unwrap();

    assert_eq!(plan.release_tag, "v0.0.0");
    assert!(plan.identity.contains("bundle=m80-linux-x86_64.tar.gz"));
    assert!(plan.identity.contains("bundle_sha256=aaaaaaaa"));
    assert!(plan.identity.contains("metadata_sha256=bbbbbbbb"));
    assert!(plan.identity.contains("index_sha256=cccccccc"));
    assert_material(
        &plan,
        "bundle",
        "m80-linux-x86_64.tar.gz",
        "https://github.com/moradology/m80/releases/download/v0.0.0/m80-linux-x86_64.tar.gz",
        false,
    );
    assert_material(
        &plan,
        "asset-index",
        "m80-release-assets.json",
        "https://github.com/moradology/m80/releases/download/v0.0.0/m80-release-assets.json",
        false,
    );
    assert_material(
        &plan,
        "install-script",
        "install.sh",
        "https://github.com/moradology/m80/releases/download/v0.0.0/install.sh",
        true,
    );
    assert_material(
        &plan,
        "release-integrity-predicate",
        "m80-release-integrity.json",
        "https://github.com/moradology/m80/releases/download/v0.0.0/m80-release-integrity.json",
        true,
    );
    assert_material(
        &plan,
        "release-attestation-bundle",
        "m80-release-integrity.attestation.jsonl",
        "https://github.com/moradology/m80/releases/download/v0.0.0/m80-release-integrity.attestation.jsonl",
        true,
    );
    assert_material(
        &plan,
        "release-attestation-metadata",
        "m80-release-attestation.json",
        "https://github.com/moradology/m80/releases/download/v0.0.0/m80-release-attestation.json",
        true,
    );
    assert_eq!(plan.materials.len(), 16);
}

#[test]
fn direct_plan_requires_official_attestation_bundle_ref() {
    let mut material = valid_material();
    material.attestation_name = None;

    let err = ReleaseMaterialPlan::from_index_material(material).unwrap_err();

    let message = err.to_string();
    assert!(
        message.contains("release material plan failed"),
        "{message}"
    );
    assert!(message.contains("field=attestation_name"), "{message}");
}

#[test]
fn direct_plan_rejects_material_name_url_injection() {
    let mut material = valid_material();
    material.metadata_name = "m80-linux-x86_64.bundle.json?download=1".to_owned();

    let err = ReleaseMaterialPlan::from_index_material(material).unwrap_err();

    let message = err.to_string();
    assert!(message.contains("asset name is invalid"), "{message}");
    assert!(message.contains("?download=1"), "{message}");
}

#[test]
fn official_release_missing_material_fails_before_staging_or_bundle_download() {
    let _guard = super::super::INSTALL_PREFLIGHT_ENV_LOCK.lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let install_root = temp.path().join("install-root");
    let material_dir = temp.path().join("materials");
    fs::create_dir(&material_dir).unwrap();
    let log_path = temp.path().join("curl.log");
    let bin_dir = temp.path().join("bin");
    fs::create_dir(&bin_dir).unwrap();
    write_fake_curl(&bin_dir);
    write_direct_release_materials(&material_dir, Some("m80-release-integrity.json"));

    let _gh_env = EnvVarGuard::set(
        "M80_RELEASE_ATTESTATION_GH",
        &fake_gh_fixture("fake-gh-attestation-supported.sh"),
    );
    let _path_env = EnvVarGuard::prepend_path(&bin_dir);
    let _material_env = EnvVarGuard::set("M80_FAKE_CURL_MATERIAL_DIR", &material_dir);
    let _log_env = EnvVarGuard::set("M80_FAKE_CURL_LOG", &log_path);

    let err =
        super::super::install_bundle_layout(&official_release_plan(&install_root)).unwrap_err();

    let message = err.to_string();
    assert!(message.contains("release material"), "{message}");
    assert!(message.contains("release_tag=v0.0.0"), "{message}");
    assert!(
        message.contains("material_class=release-integrity-predicate"),
        "{message}"
    );
    assert!(message.contains("m80-release-integrity.json"), "{message}");
    assert!(
        !install_root.exists(),
        "missing release material must fail before staging creates the install root"
    );
    let log = fs::read_to_string(log_path).unwrap();
    let bundle_url = crate::release_urls::release_asset_url("v0.0.0", "m80-linux-x86_64.tar.gz");
    assert!(
        !log.lines().any(|line| line == bundle_url),
        "bundle tarball must not be downloaded before release material is complete: {log}"
    );
}

fn assert_material(plan: &ReleaseMaterialPlan, class: &str, name: &str, url: &str, probe: bool) {
    let material = plan
        .materials
        .iter()
        .find(|material| material.class == class)
        .unwrap_or_else(|| panic!("missing material class {class}"));
    assert_eq!(material.name, name);
    assert_eq!(material.url, url);
    assert_eq!(material.probe, probe);
}

fn valid_material() -> crate::release_asset_index::DirectBundleIndexMaterial {
    crate::release_asset_index::DirectBundleIndexMaterial {
        release_tag: "v0.0.0".to_owned(),
        bundle_name: "m80-linux-x86_64.tar.gz".to_owned(),
        bundle_url: crate::release_urls::release_asset_url("v0.0.0", "m80-linux-x86_64.tar.gz"),
        bundle_sha256: "a".repeat(64),
        bundle_size_bytes: 42,
        metadata_name: "m80-linux-x86_64.bundle.json".to_owned(),
        metadata_sha256: "b".repeat(64),
        checksum_name: "m80-linux-x86_64.tar.gz.sha256".to_owned(),
        attestation_name: Some("m80-release-integrity.attestation.jsonl".to_owned()),
        target: "linux-x86_64".to_owned(),
        image_kind: "minimal".to_owned(),
        m80_version: "v0.0.0".to_owned(),
        index_url: crate::release_urls::release_asset_url("v0.0.0", "m80-release-assets.json"),
        index_checksum_url: crate::release_urls::release_asset_url(
            "v0.0.0",
            "m80-release-assets.json.sha256",
        ),
        index_expected_sha256: "c".repeat(64),
        index_observed_sha256: "c".repeat(64),
    }
}

fn official_release_plan(install_root: &Path) -> InstallPlan {
    let bundle_url = crate::release_urls::release_asset_url("v0.0.0", "m80-linux-x86_64.tar.gz");
    InstallPlan {
        dry_run: false,
        install_root: install_root.display().to_string(),
        active_pointer: install_root.join("active").display().to_string(),
        source: SourcePlan {
            kind: SourceKind::BundleUrl,
            selector: bundle_url.clone(),
            release_tag: Some("v0.0.0".to_owned()),
            bundle_url: Some(bundle_url),
        },
        binary_version: "v0.0.0".to_owned(),
        binary_release_tag: Some("v0.0.0".to_owned()),
        version_status: "release".to_owned(),
    }
}

fn write_fake_curl(bin_dir: &Path) -> PathBuf {
    let path = bin_dir.join("curl");
    fs::write(
        &path,
        r#"#!/bin/sh
set -eu
out=
url=
while [ "$#" -gt 0 ]; do
    case "$1" in
        -o)
            out=$2
            shift 2
            ;;
        -w|--connect-timeout|--max-time|--proto|--proto-redir)
            shift 2
            ;;
        -fsSL)
            shift
            ;;
        *)
            url=$1
            shift
            ;;
    esac
done
if [ -z "$out" ] || [ -z "$url" ]; then
    echo "fake curl missing -o or url" >&2
    exit 2
fi
printf '%s\n' "$url" >> "$M80_FAKE_CURL_LOG"
name=${url##*/}
if [ "$name" = "m80-linux-x86_64.tar.gz" ]; then
    echo "bundle tarball fetch is not part of release material preflight" >&2
    exit 99
fi
src="$M80_FAKE_CURL_MATERIAL_DIR/$name"
if [ ! -f "$src" ]; then
    echo "fake curl missing $name" >&2
    exit 22
fi
cp "$src" "$out"
printf '%s' "$url"
"#,
    )
    .unwrap();
    let mut permissions = fs::metadata(&path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&path, permissions).unwrap();
    path
}

fn write_direct_release_materials(material_dir: &Path, omit: Option<&str>) {
    let bundle_name = "m80-linux-x86_64.tar.gz";
    let bundle_sha256 = "a".repeat(64);
    let metadata_name = "m80-linux-x86_64.bundle.json";
    let metadata_bytes = b"{\"schema_version\":1}\n";
    let metadata_sha256 = sha256_bytes(metadata_bytes);
    write_material(material_dir, metadata_name, metadata_bytes, omit);
    write_material(
        material_dir,
        &format!("{metadata_name}.sha256"),
        format!("{metadata_sha256}  {metadata_name}\n").as_bytes(),
        omit,
    );
    write_material(
        material_dir,
        &format!("{bundle_name}.sha256"),
        format!("{bundle_sha256}  {bundle_name}\n").as_bytes(),
        omit,
    );
    for name in [
        "install.sh",
        "install.sh.sha256",
        "m80-bootstrap-selector.tsv",
        "m80-bootstrap-selector.tsv.sha256",
        "m80-release-build.json",
        "m80-release-build.json.sha256",
        "m80-release-integrity.json",
        "m80-release-integrity.attestation.jsonl",
        "m80-release-attestation.json",
        "SHA256SUMS",
    ] {
        write_material(
            material_dir,
            name,
            format!("material {name}\n").as_bytes(),
            omit,
        );
    }

    let index = serde_json::json!({
        "schema_version": 1,
        "release_tag": "v0.0.0",
        "assets": [{
            "name": bundle_name,
            "url": crate::release_urls::release_asset_url("v0.0.0", bundle_name),
            "sha256": bundle_sha256,
            "size_bytes": 42,
            "metadata_name": metadata_name,
            "metadata_sha256": metadata_sha256,
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
    write_material(material_dir, "m80-release-assets.json", &index_bytes, omit);
    write_material(
        material_dir,
        "m80-release-assets.json.sha256",
        format!("{index_sha256}  m80-release-assets.json\n").as_bytes(),
        omit,
    );
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

fn fake_gh_fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

struct EnvVarGuard {
    key: &'static str,
    previous: Option<OsString>,
}

impl EnvVarGuard {
    fn set(key: &'static str, value: &Path) -> Self {
        let previous = std::env::var_os(key);
        std::env::set_var(key, value);
        Self { key, previous }
    }

    fn prepend_path(path: &Path) -> Self {
        let previous = std::env::var_os("PATH");
        let mut value = OsString::from(path.as_os_str());
        if let Some(existing) = &previous {
            value.push(":");
            value.push(existing);
        }
        std::env::set_var("PATH", &value);
        Self {
            key: "PATH",
            previous,
        }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        match &self.previous {
            Some(value) => std::env::set_var(self.key, value),
            None => std::env::remove_var(self.key),
        }
    }
}
