use std::fs;
use std::path::Path;

use m80_firecracker::FcError;

use super::test_env::{
    fake_gh_fixture, official_release_plan, valid_material, write_fake_curl, EnvVarGuard,
};
use super::test_fixture::{
    verifier_error, verifier_error_with_curl_log, write_direct_release_materials_with,
    ReleaseFixtureOptions,
};
use super::*;

mod contract;
mod diagnostics;
mod fixture_harness;
mod no_write;

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
    let _gh_env = EnvVarGuard::set(
        "M80_RELEASE_ATTESTATION_GH",
        &fake_gh_fixture("fake-gh-attestation-supported.sh"),
    );

    let verified = super::verify_official_release_bundle(&fixture.bundle_url)
        .unwrap()
        .unwrap();

    assert_eq!(
        sha256_file(verified.bundle_path()).unwrap(),
        fixture.bundle_sha256
    );
    assert_eq!(verified.summary.release_tag, "v0.0.0");
    assert_eq!(verified.summary.bundle_asset, "m80-linux-x86_64.tar.gz");
    assert_eq!(verified.summary.bundle_url, fixture.bundle_url);
    assert_eq!(verified.summary.bundle_sha256, fixture.bundle_sha256);
    assert_eq!(verified.summary.install_sh_sha256, fixture.install_sha256);
    assert_eq!(verified.summary.predicate_sha256, fixture.predicate_sha256);
    assert_eq!(
        verified.summary.asset_index_sha256,
        fixture.asset_index_sha256
    );
    assert_eq!(
        verified.summary.attestation_signer,
        "moradology/m80/.github/workflows/release-artifacts.yml"
    );
    assert_eq!(
        verified.summary.attestation_issuer,
        "https://token.actions.githubusercontent.com"
    );
    assert_eq!(
        verified.summary.source_commit,
        "0123456789abcdef0123456789abcdef01234567"
    );
    assert!(verified.bundle_path().is_file());
}

#[test]
fn official_release_verifier_invokes_gh_attestation_verify_before_bundle_download() {
    let _guard = super::super::INSTALL_PREFLIGHT_ENV_LOCK.lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let material_dir = temp.path().join("materials");
    fs::create_dir(&material_dir).unwrap();
    let log_path = temp.path().join("curl.log");
    let gh_argv_path = temp.path().join("gh.argv");
    let gh_marker_path = temp.path().join("gh.marker");
    let bin_dir = temp.path().join("bin");
    fs::create_dir(&bin_dir).unwrap();
    write_fake_curl(&bin_dir);
    let fixture =
        write_direct_release_materials_with(&material_dir, ReleaseFixtureOptions::default());

    let _path_env = EnvVarGuard::prepend_path(&bin_dir);
    let _material_env = EnvVarGuard::set("M80_FAKE_CURL_MATERIAL_DIR", &material_dir);
    let _log_env = EnvVarGuard::set("M80_FAKE_CURL_LOG", &log_path);
    let _gh_env = EnvVarGuard::set(
        "M80_RELEASE_ATTESTATION_GH",
        &fake_gh_fixture("fake-gh-attestation-supported.sh"),
    );
    let _gh_argv_env = EnvVarGuard::set("M80_FAKE_GH_ARGV", &gh_argv_path);
    let _gh_marker_env = EnvVarGuard::set("M80_FAKE_GH_MARKER", &gh_marker_path);
    let _bundle_gate_env = EnvVarGuard::set(
        "M80_FAKE_CURL_REQUIRE_GH_MARKER_BEFORE_BUNDLE",
        &gh_marker_path,
    );

    let verified = super::verify_official_release_bundle(&fixture.bundle_url)
        .unwrap()
        .unwrap();

    assert_eq!(verified.summary.release_tag, "v0.0.0");
    assert_eq!(verified.summary.bundle_asset, "m80-linux-x86_64.tar.gz");
    assert_eq!(verified.summary.bundle_sha256, fixture.bundle_sha256);
    assert_eq!(verified.summary.install_sh_sha256, fixture.install_sha256);
    assert_eq!(verified.summary.predicate_sha256, fixture.predicate_sha256);
    assert_eq!(
        verified.summary.asset_index_sha256,
        fixture.asset_index_sha256
    );
    assert_eq!(
        verified.summary.attestation_signer,
        "moradology/m80/.github/workflows/release-artifacts.yml"
    );
    assert_eq!(
        verified.summary.attestation_issuer,
        "https://token.actions.githubusercontent.com"
    );
    assert_eq!(
        verified.summary.source_commit,
        "0123456789abcdef0123456789abcdef01234567"
    );

    let argv = fs::read_to_string(&gh_argv_path).unwrap();
    let lines = argv.lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), 18, "{argv}");
    assert_eq!(lines[0], "attestation");
    assert_eq!(lines[1], "verify");
    assert!(lines[2].ends_with("m80-release-integrity.json"), "{argv}");
    assert_eq!(lines[3], "--repo");
    assert_eq!(lines[4], "moradology/m80");
    assert_eq!(lines[5], "--bundle");
    assert!(
        lines[6].ends_with("m80-release-integrity.attestation.jsonl"),
        "{argv}"
    );
    assert_eq!(lines[7], "--signer-workflow");
    assert_eq!(
        lines[8],
        "moradology/m80/.github/workflows/release-artifacts.yml"
    );
    assert_eq!(lines[9], "--cert-oidc-issuer");
    assert_eq!(lines[10], "https://token.actions.githubusercontent.com");
    assert_eq!(lines[11], "--source-ref");
    assert_eq!(lines[12], "refs/tags/v0.0.0");
    assert_eq!(lines[13], "--source-digest");
    assert_eq!(lines[14], "0123456789abcdef0123456789abcdef01234567");
    assert_eq!(lines[15], "--deny-self-hosted-runners");
    assert_eq!(lines[16], "--format");
    assert_eq!(lines[17], "json");

    let log = fs::read_to_string(&log_path).unwrap();
    let bundle_url = crate::release_urls::release_asset_url("v0.0.0", "m80-linux-x86_64.tar.gz");
    assert!(
        log.lines().any(|line| line == bundle_url),
        "successful verification should download the bundle after gh verification: {log}"
    );
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
fn official_release_verifier_rejects_failed_cryptographic_attestation_before_bundle_download() {
    let (err, log) = verifier_error_with_curl_log(ReleaseFixtureOptions {
        gh_failure: true,
        ..ReleaseFixtureOptions::default()
    });

    let message = err.to_string();
    assert!(
        message.contains("cryptographic attestation verification failed"),
        "{message}"
    );
    assert!(
        message.contains("cryptographic attestation invalid"),
        "{message}"
    );
    assert_no_bundle_download(&log);
}

#[test]
fn official_release_verifier_rejects_wrong_attestation_subject_before_bundle_download() {
    let (err, log) = verifier_error_with_curl_log(ReleaseFixtureOptions {
        gh_wrong_subject_digest: true,
        ..ReleaseFixtureOptions::default()
    });

    let message = err.to_string();
    assert!(
        message.contains("JSON omitted release-integrity predicate name/sha256 subject"),
        "{message}"
    );
    assert_no_bundle_download(&log);
}

#[test]
fn official_release_verifier_rejects_wrong_commit_digest_before_bundle_download() {
    let (err, log) = verifier_error_with_curl_log(ReleaseFixtureOptions {
        wrong_commit_sha: true,
        ..ReleaseFixtureOptions::default()
    });

    let message = err.to_string();
    assert!(
        message.contains("cryptographic attestation verification failed"),
        "{message}"
    );
    assert!(message.contains("source digest mismatch"), "{message}");
    assert_no_bundle_download(&log);
}

#[test]
fn official_release_verifier_rejects_wrong_source_ref_before_bundle_download() {
    let (err, log) = verifier_error_with_curl_log(ReleaseFixtureOptions {
        gh_wrong_source_ref: true,
        ..ReleaseFixtureOptions::default()
    });

    let message = err.to_string();
    assert!(
        message.contains("cryptographic attestation verification failed"),
        "{message}"
    );
    assert!(message.contains("source ref mismatch"), "{message}");
    assert_no_bundle_download(&log);
}

#[test]
fn official_release_verifier_rejects_wrong_attestation_signer_before_bundle_download() {
    let (err, log) = verifier_error_with_curl_log(ReleaseFixtureOptions {
        wrong_attestation_signer: true,
        ..ReleaseFixtureOptions::default()
    });

    let message = err.to_string();
    assert!(message.contains("signer_identity"), "{message}");
    assert!(message.contains("mismatch"), "{message}");
    assert_no_bundle_download(&log);
}

#[test]
fn official_release_verifier_rejects_wrong_attestation_issuer_before_bundle_download() {
    let (err, log) = verifier_error_with_curl_log(ReleaseFixtureOptions {
        wrong_attestation_issuer: true,
        ..ReleaseFixtureOptions::default()
    });

    let message = err.to_string();
    assert!(message.contains("release attestation issuer"), "{message}");
    assert!(message.contains("mismatch"), "{message}");
    assert_no_bundle_download(&log);
}

#[test]
fn official_release_verifier_rejects_wrong_attestation_keyset_before_bundle_download() {
    let (err, log) = verifier_error_with_curl_log(ReleaseFixtureOptions {
        wrong_attestation_keyset: true,
        ..ReleaseFixtureOptions::default()
    });

    let message = err.to_string();
    assert!(
        message.contains("release attestation keyset_id"),
        "{message}"
    );
    assert!(message.contains("mismatch"), "{message}");
    assert_no_bundle_download(&log);
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

#[test]
fn official_release_verifier_accepts_github_cdn_redirect_for_expected_material() {
    let verified = verifier_result_with_redirect_rows(&[(
        "install.sh",
        "https://objects.githubusercontent.com/github-production-release-asset/expected-install",
        "install.sh",
    )])
    .unwrap();

    assert_eq!(verified.summary.release_tag, "v0.0.0");
    assert_eq!(verified.summary.bundle_asset, "m80-linux-x86_64.tar.gz");
}

#[test]
fn official_release_verifier_rejects_github_cdn_redirect_with_wrong_role_bytes() {
    let err = verifier_error_with_redirect_rows(&[(
        "install.sh",
        "https://objects.githubusercontent.com/github-production-release-asset/expected-install",
        "m80-bootstrap-selector.tsv",
    )]);

    let message = err.to_string();
    assert!(
        message.contains("release material public SHA256SUMS mismatch")
            || message.contains("release integrity sha256 mismatch")
            || message.contains("release material sidecar digest mismatch"),
        "{message}"
    );
    assert!(
        message.contains("material_class=install-script"),
        "{message}"
    );
}

#[test]
fn official_release_verifier_rejects_redirect_to_foreign_repo_release_asset() {
    let err = verifier_error_with_redirect_rows(&[(
        "install.sh",
        "https://github.com/attacker/m80/releases/download/v0.0.0/install.sh",
        "install.sh",
    )]);

    assert_redirect_identity_error(err, "install-script", "install.sh", "repository");
}

#[test]
fn official_release_verifier_rejects_redirect_to_wrong_release_tag() {
    let err = verifier_error_with_redirect_rows(&[(
        "install.sh",
        "https://github.com/moradology/m80/releases/download/v9.9.9/install.sh",
        "install.sh",
    )]);

    assert_redirect_identity_error(err, "install-script", "install.sh", "release_tag");
}

#[test]
fn official_release_verifier_rejects_redirect_to_wrong_asset_name() {
    let err = verifier_error_with_redirect_rows(&[(
        "install.sh",
        "https://github.com/moradology/m80/releases/download/v0.0.0/not-install.sh",
        "install.sh",
    )]);

    assert_redirect_identity_error(err, "install-script", "install.sh", "asset_name");
}

#[test]
fn official_release_verifier_rejects_redirect_to_github_cdn_host_lookalike() {
    let err = verifier_error_with_redirect_rows(&[(
        "install.sh",
        "https://objects.githubusercontent.com.evil.invalid/github-production-release-asset/install",
        "install.sh",
    )]);

    assert_redirect_identity_error(err, "install-script", "install.sh", "host");
}

#[test]
fn official_release_verifier_rejects_digest_matching_wrong_role_redirect() {
    let err = verifier_error_with_redirect_rows(&[(
        "install.sh",
        "https://github.com/moradology/m80/releases/download/v0.0.0/m80-bootstrap-selector.tsv",
        "install.sh",
    )]);

    assert_redirect_identity_error(err, "install-script", "install.sh", "asset_name");
}

fn assert_no_bundle_download(log: &str) {
    let bundle_url = crate::release_urls::release_asset_url("v0.0.0", "m80-linux-x86_64.tar.gz");
    assert!(
        !log.lines().any(|line| line == bundle_url),
        "bundle tarball must not be downloaded after release trust failure: {log}"
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

fn verifier_error_with_redirect_rows(rows: &[(&str, &str, &str)]) -> FcError {
    verifier_result_with_redirect_rows(rows).unwrap_err()
}

fn verifier_result_with_redirect_rows(
    rows: &[(&str, &str, &str)],
) -> Result<VerifiedOfficialReleaseBundle, FcError> {
    let _guard = super::super::INSTALL_PREFLIGHT_ENV_LOCK.lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let material_dir = temp.path().join("materials");
    fs::create_dir(&material_dir).unwrap();
    let log_path = temp.path().join("curl.log");
    let redirect_map = temp.path().join("redirect.map");
    let bin_dir = temp.path().join("bin");
    fs::create_dir(&bin_dir).unwrap();
    write_fake_curl(&bin_dir);
    let fixture =
        write_direct_release_materials_with(&material_dir, ReleaseFixtureOptions::default());
    write_redirect_map(&redirect_map, rows);

    let _path_env = EnvVarGuard::prepend_path(&bin_dir);
    let _material_env = EnvVarGuard::set("M80_FAKE_CURL_MATERIAL_DIR", &material_dir);
    let _log_env = EnvVarGuard::set("M80_FAKE_CURL_LOG", &log_path);
    let _redirect_env = EnvVarGuard::set("M80_FAKE_CURL_REDIRECT_MAP", &redirect_map);
    let _gh_env = EnvVarGuard::set(
        "M80_RELEASE_ATTESTATION_GH",
        &fake_gh_fixture("fake-gh-attestation-supported.sh"),
    );

    super::verify_official_release_bundle(&fixture.bundle_url).map(|verified| {
        verified.expect("fixture bundle URL should classify as an official release bundle")
    })
}

fn write_redirect_map(path: &Path, rows: &[(&str, &str, &str)]) {
    let mut text = String::new();
    for (name, final_url, source_name) in rows {
        text.push_str(name);
        text.push('\t');
        text.push_str(final_url);
        text.push('\t');
        text.push_str(source_name);
        text.push('\n');
    }
    fs::write(path, text).unwrap();
}

fn assert_redirect_identity_error(err: FcError, role: &str, asset_name: &str, field: &str) {
    let message = err.to_string();
    assert!(
        message.contains("release material redirect identity mismatch"),
        "{message}"
    );
    assert!(
        message.contains(&format!("material_role={role}")),
        "{message}"
    );
    assert!(message.contains("requested_url="), "{message}");
    assert!(message.contains("final_url="), "{message}");
    assert!(
        message.contains(&format!("expected_asset_name={asset_name}")),
        "{message}"
    );
    assert!(
        message.contains(&format!("rejected_identity_field={field}")),
        "{message}"
    );
}
