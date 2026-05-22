use std::fs;
use std::path::PathBuf;

use serde_json::Value;

#[path = "../common/mod.rs"]
mod common;

use common::m80;

const RELEASE_TAG: &str = concat!("v", env!("CARGO_PKG_VERSION"));

#[test]
fn install_dry_run_bundle_url_does_not_touch_install_root() {
    let temp = tempfile::tempdir().unwrap();
    let install_root = temp.path().join("install-root");

    let output = m80()
        .args([
            "install",
            "--bundle-url",
            "file:///tmp/m80-linux-x86_64.tar.gz",
            "--install-root",
            install_root.to_str().unwrap(),
            "--dry-run",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "install dry-run failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("source_kind=bundle_url"), "{stdout}");
    assert!(stdout.contains("dry_run=true"), "{stdout}");
    assert!(stdout.contains("writes=none"), "{stdout}");
    assert!(
        !install_root.exists(),
        "dry-run must not create install root {}",
        install_root.display()
    );
}

#[test]
fn install_json_dry_run_uses_stdout_envelope() {
    let temp = tempfile::tempdir().unwrap();
    let install_root = temp.path().join("install-root");

    let output = m80()
        .args([
            "--json",
            "install",
            "--bundle-url",
            "file:///tmp/m80-linux-x86_64.tar.gz",
            "--install-root",
            install_root.to_str().unwrap(),
            "--dry-run",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "install --json dry-run failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["version"], 1);
    assert_eq!(value["data"]["dry_run"], true);
    assert_eq!(value["data"]["source"]["kind"], "bundle_url");
    assert_eq!(
        value["data"]["source"]["bundle_url"],
        "file:///tmp/m80-linux-x86_64.tar.gz"
    );
    assert_eq!(
        value["data"]["install_root"],
        install_root.display().to_string()
    );
    assert!(
        !install_root.exists(),
        "JSON dry-run must not create install root {}",
        install_root.display()
    );
}

#[test]
fn install_release_tag_refuses_dev_build_before_install_root_touch() {
    let temp = tempfile::tempdir().unwrap();
    let install_root = temp.path().join("install-root");

    let output = m80()
        .args([
            "install",
            "--release-tag",
            RELEASE_TAG,
            "--install-root",
            install_root.to_str().unwrap(),
            "--dry-run",
        ])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(6));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("requires a tagged m80 binary"),
        "unexpected stderr: {stderr}"
    );
    assert!(
        stderr.contains("asset_index_code=dev_build_refused"),
        "{stderr}"
    );
    assert!(
        stderr.contains(&format!("requested_release_tag={RELEASE_TAG}")),
        "{stderr}"
    );
    assert!(stderr.contains("requested_image_kind=minimal"), "{stderr}");
    assert!(
        stderr.contains("repair_command=m80 install --bundle-url <compatible-bundle-url>"),
        "{stderr}"
    );
    assert!(
        !install_root.exists(),
        "dev-build refusal must not create install root {}",
        install_root.display()
    );
}

#[test]
fn install_json_release_tag_refusal_reports_asset_index_fields_on_stderr() {
    let temp = tempfile::tempdir().unwrap();
    let install_root = temp.path().join("install-root");

    let output = m80()
        .args([
            "--json",
            "install",
            "--release-tag",
            RELEASE_TAG,
            "--install-root",
            install_root.to_str().unwrap(),
            "--dry-run",
        ])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(6));
    assert!(
        output.stdout.is_empty(),
        "asset-index JSON failures must keep stdout empty: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let value: Value = serde_json::from_slice(&output.stderr)
        .unwrap_or_else(|err| panic!("stderr should be one JSON object: {err}"));
    assert_eq!(value["version"], 1);
    let data = &value["data"];
    assert_eq!(data["variant"], "ReleaseAssetIndex");
    assert_eq!(data["exit_code"], 6);
    assert_eq!(data["code"], "dev_build_refused");
    assert_eq!(data["requested_os"], std::env::consts::OS);
    assert_eq!(data["requested_arch"], std::env::consts::ARCH);
    assert_eq!(data["requested_image_kind"], "minimal");
    assert_eq!(data["requested_release_tag"], RELEASE_TAG);
    assert!(
        data["requested_m80_version"]
            .as_str()
            .expect("requested_m80_version")
            .ends_with("-dev"),
        "unexpected requested_m80_version: {data}"
    );
    assert_eq!(data["available_tuples"].as_array().map(Vec::len), Some(0));
    assert_eq!(
        data["repair_command"],
        "m80 install --bundle-url <compatible-bundle-url>"
    );
    assert!(
        !install_root.exists(),
        "dev-build refusal must not create install root {}",
        install_root.display()
    );
}

#[test]
fn install_missing_source_prints_source_diagnostic() {
    let output = m80().args(["install", "--dry-run"]).output().unwrap();

    assert_eq!(output.status.code(), Some(6));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("choose exactly one source")
            && stderr.contains("--release-tag <TAG>")
            && stderr.contains("--bundle-url <URL>")
            && !stderr.contains("--bootstrap-tag"),
        "unexpected stderr: {stderr}"
    );
}

#[test]
fn install_non_release_remote_bundle_url_is_rejected_without_touching_install_root() {
    assert_bundle_url_rejected_before_install_root_touch(
        "https://example.invalid/m80-linux-x86_64.tar.gz",
        "moradology/m80 GitHub release bundle asset",
    );
}

#[test]
fn install_foreign_github_release_bundle_url_is_rejected_without_touching_install_root() {
    assert_bundle_url_rejected_before_install_root_touch(
        "https://github.com/example/m80/releases/download/v1.2.3/m80-linux-x86_64.tar.gz",
        "foreign repositories",
    );
}

#[test]
fn install_latest_artifact_bundle_url_is_rejected_without_touching_install_root() {
    assert_bundle_url_rejected_before_install_root_touch(
        "https://github.com/moradology/m80/releases/latest/download/m80-linux-x86_64.tar.gz",
        "latest artifact URLs",
    );
}

#[test]
fn install_prerelease_bundle_tag_is_rejected_without_touching_install_root() {
    assert_bundle_url_rejected_before_install_root_touch(
        "https://github.com/moradology/m80/releases/download/v1.2.3-rc.1/m80-linux-x86_64.tar.gz",
        "must be a concrete",
    );
}

#[test]
fn install_raw_branch_bundle_url_is_rejected_without_touching_install_root() {
    assert_bundle_url_rejected_before_install_root_touch(
        "https://raw.githubusercontent.com/moradology/m80/main/m80-linux-x86_64.tar.gz",
        "raw branch URLs",
    );
}

#[test]
fn install_bad_release_asset_name_is_rejected_without_touching_install_root() {
    assert_bundle_url_rejected_before_install_root_touch(
        "https://github.com/moradology/m80/releases/download/v1.2.3/install.sh",
        "non-bundle assets",
    );
}

#[test]
fn install_path_traversal_release_asset_url_is_rejected_without_touching_install_root() {
    assert_bundle_url_rejected_before_install_root_touch(
        "https://github.com/moradology/m80/releases/download/v1.2.3/../m80-linux-x86_64.tar.gz",
        "non-bundle assets",
    );
}

#[test]
fn install_non_https_github_release_url_is_rejected_without_touching_install_root() {
    assert_bundle_url_rejected_before_install_root_touch(
        "http://github.com/moradology/m80/releases/download/v1.2.3/m80-linux-x86_64.tar.gz",
        "must be a concrete",
    );
}

#[test]
fn installer_input_contract_doc_names_source_shapes_and_tests() {
    let doc = read_repo_file("docs/behaviors/release/installer-input-contract.md");

    for required in [
        "`--release-tag <TAG>`",
        "`--bundle-url <URL>`",
        "`--bootstrap-tag <TAG>`",
        "`--install-root <PATH>`",
        "`--dry-run`",
        "exactly one",
        "does not create",
        "install_dry_run_bundle_url_does_not_touch_install_root",
        "install_json_dry_run_uses_stdout_envelope",
        "install_release_tag_refuses_dev_build_before_install_root_touch",
        "install_json_release_tag_refusal_reports_asset_index_fields_on_stderr",
        "install_missing_source_prints_source_diagnostic",
        "install_non_release_remote_bundle_url_is_rejected_without_touching_install_root",
        "install_foreign_github_release_bundle_url_is_rejected_without_touching_install_root",
        "install_latest_artifact_bundle_url_is_rejected_without_touching_install_root",
        "install_prerelease_bundle_tag_is_rejected_without_touching_install_root",
        "install_raw_branch_bundle_url_is_rejected_without_touching_install_root",
        "install_bad_release_asset_name_is_rejected_without_touching_install_root",
        "install_path_traversal_release_asset_url_is_rejected_without_touching_install_root",
        "install_non_https_github_release_url_is_rejected_without_touching_install_root",
    ] {
        assert!(
            doc.contains(required),
            "installer input contract doc missing {required:?}"
        );
    }
}

#[test]
fn installer_input_trust_model_doc_names_direct_url_boundaries() {
    let doc = read_repo_file("docs/behaviors/release/installer-input.md");

    for required in [
        "`curl -fsSL https://github.com/moradology/m80/releases/latest/download/install.sh | sudo sh`",
        "Public installer status: pending until the unauthenticated public-access proof",
        "`curl -fsSL https://github.com/moradology/m80/releases/download/<version>/install.sh | sudo sh`",
        "`m80 install --release-tag <tag>`",
        "`m80 install --bundle-url https://github.com/moradology/m80/releases/download/v1.2.3/m80-linux-x86_64.tar.gz`",
        "same-tag public integrity material",
        "not a checksum-only compatibility mode",
        "Local `file://` bundles and local fixture HTTP URLs are operator/test overrides",
        "`releases/latest/download/<bundle>.tar.gz` shape",
        "`https://github.com/example/m80/releases/download/v1.2.3/m80-linux-x86_64.tar.gz`",
        "raw branch installer URLs",
        "fail before network access or install-root mutation",
        "[`direct-url-diagnostics.md`](direct-url-diagnostics.md)",
    ] {
        assert!(
            doc.contains(required),
            "installer input trust model doc missing {required:?}"
        );
    }
}

fn read_repo_file(relative: &str) -> String {
    fs::read_to_string(repo_root().join(relative)).expect("read repository file")
}

fn assert_bundle_url_rejected_before_install_root_touch(bundle_url: &str, expected: &str) {
    let temp = tempfile::tempdir().unwrap();
    let install_root = temp.path().join("install-root");

    let output = m80()
        .args([
            "install",
            "--bundle-url",
            bundle_url,
            "--install-root",
            install_root.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(6));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains(expected),
        "expected {expected:?} in stderr: {stderr}"
    );
    assert!(
        stderr.contains("m80 install --release-tag <tag>"),
        "stderr should point to the normal public install path: {stderr}"
    );
    assert!(
        !install_root.exists(),
        "unsupported non-file bundle URL must not create install root {}",
        install_root.display()
    );
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("m80-cli crate should be under crates/")
        .to_path_buf()
}
