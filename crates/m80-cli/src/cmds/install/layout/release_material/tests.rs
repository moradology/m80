use std::fs;

use super::test_env::{
    fake_gh_fixture, official_release_plan, valid_material, write_fake_curl, EnvVarGuard,
};
use super::test_fixture::{
    verifier_error, write_direct_release_materials_with, ReleaseFixtureOptions,
};
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
    write_direct_release_materials_with(
        &material_dir,
        ReleaseFixtureOptions {
            omit: Some("m80-release-integrity.json"),
            ..ReleaseFixtureOptions::default()
        },
    );

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

#[test]
fn official_release_verifier_accepts_complete_material_before_staging() {
    let _guard = super::super::INSTALL_PREFLIGHT_ENV_LOCK.lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let material_dir = temp.path().join("materials");
    fs::create_dir(&material_dir).unwrap();
    let log_path = temp.path().join("curl.log");
    let bin_dir = temp.path().join("bin");
    fs::create_dir(&bin_dir).unwrap();
    write_fake_curl(&bin_dir);
    let fixture =
        write_direct_release_materials_with(&material_dir, ReleaseFixtureOptions::default());

    let _path_env = EnvVarGuard::prepend_path(&bin_dir);
    let _material_env = EnvVarGuard::set("M80_FAKE_CURL_MATERIAL_DIR", &material_dir);
    let _log_env = EnvVarGuard::set("M80_FAKE_CURL_LOG", &log_path);

    let verified = super::verify_official_release_bundle(&fixture.bundle_url)
        .unwrap()
        .unwrap();

    assert_eq!(
        sha256_file(verified.bundle_path()).unwrap(),
        fixture.bundle_sha256
    );
    assert_eq!(verified.summary.install_sh_sha256, fixture.install_sha256);
    assert_eq!(verified.summary.predicate_sha256, fixture.predicate_sha256);
    assert_eq!(
        verified.summary.attestation_signer,
        "moradology/m80/.github/workflows/release-artifacts.yml"
    );
    assert!(verified.bundle_path().is_file());
}

#[test]
fn official_release_verifier_rejects_tampered_bundle_bytes() {
    let err = verifier_error(ReleaseFixtureOptions {
        tamper_bundle: true,
        ..ReleaseFixtureOptions::default()
    });

    let message = err.to_string();
    assert!(
        message.contains("bundle digest mismatch") || message.contains("sidecar digest mismatch"),
        "{message}"
    );
    assert!(message.contains("bundle"), "{message}");
}

#[test]
fn official_release_verifier_rejects_wrong_bundle_checksum_row() {
    let err = verifier_error(ReleaseFixtureOptions {
        wrong_bundle_checksum: true,
        ..ReleaseFixtureOptions::default()
    });

    let message = err.to_string();
    assert!(
        message.contains("checksum mismatch") || message.contains("sidecar digest mismatch"),
        "{message}"
    );
    assert!(message.contains("bundle-checksum"), "{message}");
}

#[test]
fn official_release_verifier_rejects_stale_asset_index_row() {
    let err = verifier_error(ReleaseFixtureOptions {
        stale_asset_index: true,
        ..ReleaseFixtureOptions::default()
    });

    let message = err.to_string();
    assert!(
        message.contains("bundle digest mismatch") || message.contains("checksum mismatch"),
        "{message}"
    );
    assert!(message.contains("bundle-checksum"), "{message}");
}

#[test]
fn official_release_verifier_rejects_mismatched_attestation_metadata() {
    let err = verifier_error(ReleaseFixtureOptions {
        mismatched_attestation: true,
        ..ReleaseFixtureOptions::default()
    });

    let message = err.to_string();
    assert!(message.contains("predicate_sha256"), "{message}");
    assert!(message.contains("mismatch"), "{message}");
}

#[test]
fn official_release_verifier_rejects_missing_install_sh_digest() {
    let err = verifier_error(ReleaseFixtureOptions {
        missing_install_digest: true,
        ..ReleaseFixtureOptions::default()
    });

    let message = err.to_string();
    assert!(message.contains("public SHA256SUMS"), "{message}");
    assert!(message.contains("install.sh"), "{message}");
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
