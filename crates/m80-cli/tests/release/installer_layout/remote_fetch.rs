use std::fs;
use std::path::Path;

use super::common::m80;
use super::fixture::{self, sha256_hex, write_release_bundle, RELEASE_TAG};
use super::http_fixture::{HttpFixture, TestResponse};
use super::{run_install_url, HostPrereqFixture};

#[test]
fn install_bundle_layout_downloads_http_bundle_into_version_dir() {
    let bundle = write_release_bundle(None);
    let host = HostPrereqFixture::new();
    let install_temp = tempfile::tempdir().unwrap();
    let install_root = install_temp.path().join("install-root");
    let server = HttpFixture::new([
        (
            "/m80-linux-x86_64.tar.gz",
            TestResponse::ok(fs::read(&bundle.tarball).unwrap()),
        ),
        (
            "/m80-linux-x86_64.tar.gz.sha256",
            TestResponse::ok(bundle_checksum_line(&bundle).into_bytes()),
        ),
    ]);

    let output = run_install_url(
        &server.url("/m80-linux-x86_64.tar.gz"),
        &install_root,
        Some(&host),
        &[],
        &[],
    );

    assert!(
        output.status.success(),
        "install failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("installed bundle layout"), "{stdout}");
    assert!(
        install_root
            .join("versions")
            .join(&bundle.release_tag)
            .join("bundle.json")
            .is_file(),
        "downloaded bundle should publish the version dir"
    );
}

#[test]
fn install_official_release_missing_attestation_verifier_fails_before_download_or_staging() {
    let install_temp = tempfile::tempdir().unwrap();
    let install_root = install_temp.path().join("install-root");
    let missing_gh = install_temp.path().join("missing-gh");

    let output = run_install_url(
        &format!(
            "https://github.com/moradology/m80/releases/download/{RELEASE_TAG}/m80-linux-x86_64.tar.gz"
        ),
        &install_root,
        None,
        &[("M80_RELEASE_ATTESTATION_GH", missing_gh.to_str().unwrap())],
        &[],
    );

    assert_eq!(output.status.code(), Some(6));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("release attestation verifier missing"),
        "{stderr}"
    );
    assert!(
        stderr.contains("Install or upgrade GitHub CLI with attestation support on Linux"),
        "{stderr}"
    );
    assert!(
        !install_root.exists(),
        "missing attestation verifier must fail before install root creation"
    );
}

#[test]
fn install_official_release_too_old_attestation_verifier_fails_before_download_or_staging() {
    let install_temp = tempfile::tempdir().unwrap();
    let install_root = install_temp.path().join("install-root");
    let fake_gh = fake_gh_fixture("fake-gh-attestation-too-old.sh");

    let output = run_install_url(
        &format!(
            "https://github.com/moradology/m80/releases/download/{RELEASE_TAG}/m80-linux-x86_64.tar.gz"
        ),
        &install_root,
        None,
        &[("M80_RELEASE_ATTESTATION_GH", fake_gh.to_str().unwrap())],
        &[],
    );

    assert_eq!(output.status.code(), Some(6));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("release attestation verifier unsupported"),
        "{stderr}"
    );
    assert!(stderr.contains("gh version 2.0.0"), "{stderr}");
    assert!(
        stderr.contains("unknown command \"attestation\""),
        "{stderr}"
    );
    assert!(
        !install_root.exists(),
        "too-old attestation verifier must fail before install root creation"
    );
}

#[test]
fn install_bundle_layout_rejects_remote_bundle_checksum_mismatch_before_extract() {
    let bundle = write_release_bundle(None);
    let install_temp = tempfile::tempdir().unwrap();
    let install_root = install_temp.path().join("install-root");
    let server = HttpFixture::new([
        (
            "/m80-linux-x86_64.tar.gz",
            TestResponse::ok(fs::read(&bundle.tarball).unwrap()),
        ),
        (
            "/m80-linux-x86_64.tar.gz.sha256",
            TestResponse::ok(format!("{}  m80-linux-x86_64.tar.gz\n", "0".repeat(64)).into_bytes()),
        ),
    ]);

    let output = m80()
        .args([
            "install",
            "--bundle-url",
            &server.url("/m80-linux-x86_64.tar.gz"),
            "--install-root",
            install_root.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(6));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("bundle sha256 mismatch"), "{stderr}");
    assert!(
        !install_root.join("versions").join(RELEASE_TAG).exists(),
        "checksum mismatch must not publish a version dir"
    );
    assert_no_staged_files(&install_root);
}

#[test]
fn install_bundle_layout_rejects_remote_404_before_extract() {
    let install_temp = tempfile::tempdir().unwrap();
    let install_root = install_temp.path().join("install-root");
    let server = HttpFixture::new([(
        "/m80-linux-x86_64.tar.gz",
        TestResponse::status(404, b"missing".to_vec()),
    )]);

    let output = m80()
        .args([
            "install",
            "--bundle-url",
            &server.url("/m80-linux-x86_64.tar.gz"),
            "--install-root",
            install_root.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("curl release bundle tarball"), "{stderr}");
    assert!(
        !install_root.join("versions").join(RELEASE_TAG).exists(),
        "404 must not publish a version dir"
    );
    assert_no_staged_files(&install_root);
}

#[test]
fn install_bundle_layout_deletes_truncated_download_partial() {
    let bundle = write_release_bundle(None);
    let body = fs::read(&bundle.tarball).unwrap();
    let install_temp = tempfile::tempdir().unwrap();
    let install_root = install_temp.path().join("install-root");
    let server = HttpFixture::new([(
        "/m80-linux-x86_64.tar.gz",
        TestResponse::truncated(body[..body.len() / 2].to_vec(), body.len() + 100),
    )]);

    let output = m80()
        .args([
            "install",
            "--bundle-url",
            &server.url("/m80-linux-x86_64.tar.gz"),
            "--install-root",
            install_root.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(
        !install_root.join("versions").join(RELEASE_TAG).exists(),
        "truncated download must not publish a version dir"
    );
    assert_no_staged_files(&install_root);
}

#[test]
fn install_bundle_layout_rejects_redirect_to_different_fixture_host() {
    let bundle = write_release_bundle(None);
    let install_temp = tempfile::tempdir().unwrap();
    let install_root = install_temp.path().join("install-root");
    let server = HttpFixture::new([
        (
            "/redirect.tar.gz",
            TestResponse::redirect_placeholder_host("/m80-linux-x86_64.tar.gz"),
        ),
        (
            "/m80-linux-x86_64.tar.gz",
            TestResponse::ok(fs::read(&bundle.tarball).unwrap()),
        ),
    ]);

    let output = m80()
        .args([
            "install",
            "--bundle-url",
            &server.url("/redirect.tar.gz"),
            "--install-root",
            install_root.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(6));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("redirected to unsupported host"),
        "unexpected stderr: {stderr}"
    );
    assert!(
        !install_root.join("versions").join(RELEASE_TAG).exists(),
        "unsupported redirect must not publish a version dir"
    );
    assert_no_staged_files(&install_root);
}

#[test]
fn install_bundle_layout_rejects_checksum_redirect_to_different_fixture_host() {
    let bundle = write_release_bundle(None);
    let install_temp = tempfile::tempdir().unwrap();
    let install_root = install_temp.path().join("install-root");
    let server = HttpFixture::new([
        (
            "/m80-linux-x86_64.tar.gz",
            TestResponse::ok(fs::read(&bundle.tarball).unwrap()),
        ),
        (
            "/m80-linux-x86_64.tar.gz.sha256",
            TestResponse::redirect_placeholder_host("/checksum-alt.sha256"),
        ),
        (
            "/checksum-alt.sha256",
            TestResponse::ok(bundle_checksum_line(&bundle).into_bytes()),
        ),
    ]);

    let output = m80()
        .args([
            "install",
            "--bundle-url",
            &server.url("/m80-linux-x86_64.tar.gz"),
            "--install-root",
            install_root.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(6));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("redirected to unsupported host"),
        "unexpected stderr: {stderr}"
    );
    assert!(
        !install_root.join("versions").join(RELEASE_TAG).exists(),
        "unsupported checksum redirect must not publish a version dir"
    );
    assert_no_staged_files(&install_root);
}

fn bundle_checksum_line(bundle: &fixture::ReleaseBundleFixture) -> String {
    format!("{}  m80-linux-x86_64.tar.gz\n", sha256_hex(&bundle.tarball))
}

fn fake_gh_fixture(name: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn assert_no_staged_files(install_root: &Path) {
    let staging = install_root.join(".staging");
    if !staging.exists() {
        return;
    }
    assert_no_files_under(&staging);
}

fn assert_no_files_under(path: &Path) {
    for entry in fs::read_dir(path).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        let file_type = entry.file_type().unwrap();
        if file_type.is_dir() {
            assert_no_files_under(&path);
        } else {
            panic!("staged partial file was not cleaned up: {}", path.display());
        }
    }
}
