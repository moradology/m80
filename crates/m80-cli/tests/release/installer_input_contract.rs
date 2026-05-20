use std::fs;
use std::path::PathBuf;

use serde_json::Value;

#[path = "../common/mod.rs"]
mod common;

use common::m80;

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
            "v0.0.0",
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
    let temp = tempfile::tempdir().unwrap();
    let install_root = temp.path().join("install-root");

    let output = m80()
        .args([
            "install",
            "--bundle-url",
            "https://example.invalid/m80-linux-x86_64.tar.gz",
            "--install-root",
            install_root.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(6));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("moradology/m80 GitHub release asset"),
        "unexpected stderr: {stderr}"
    );
    assert!(
        !install_root.exists(),
        "unsupported non-file bundle URL must not create install root {}",
        install_root.display()
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
        "install_missing_source_prints_source_diagnostic",
        "install_non_release_remote_bundle_url_is_rejected_without_touching_install_root",
    ] {
        assert!(
            doc.contains(required),
            "installer input contract doc missing {required:?}"
        );
    }
}

fn read_repo_file(relative: &str) -> String {
    fs::read_to_string(repo_root().join(relative)).expect("read repository file")
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("m80-cli crate should be under crates/")
        .to_path_buf()
}
