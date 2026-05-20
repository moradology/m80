//! Smoke test: `m80 version` exits 0 and prints something meaningful.

mod common;

use common::m80;

#[test]
fn version_exits_zero() {
    m80().arg("version").assert().success();
}

#[test]
fn clap_version_marks_dev_build() {
    let output = m80().arg("--version").output().unwrap();
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("-dev"),
        "plain workspace build must be visibly unreleased: {stdout}"
    );
}

#[test]
fn version_prints_binary_version() {
    let output = m80().arg("version").output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Must mention the dev-marked crate version (0.0.0-dev in this workspace).
    assert!(
        stdout.contains("0.0.0-dev") || stdout.contains("m80"),
        "version output should mention binary version: {stdout}"
    );
    assert!(
        stdout.contains("release         unreleased"),
        "dev build must not look like a release: {stdout}"
    );
}

#[test]
fn version_json_has_fields() {
    let output = m80().args(["--json", "version"]).output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let v: serde_json::Value =
        serde_json::from_str(&stdout).expect("version --json should produce valid JSON");
    assert_eq!(v["version"], 1, "missing JSON envelope version: {v}");
    assert!(
        v["data"].get("binary_version").is_some(),
        "missing binary_version field: {v}"
    );
    assert!(
        v["data"].get("protocol_version").is_some(),
        "missing protocol_version field: {v}"
    );
    assert!(
        v["data"].get("manifest_schema_version").is_some(),
        "missing manifest_schema_version field: {v}"
    );
    assert!(
        v["data"].get("build_receipt_schema_version").is_some(),
        "missing build_receipt_schema_version field: {v}"
    );
    assert!(
        v["data"].get("install_provenance_schema_version").is_some(),
        "missing install_provenance_schema_version field: {v}"
    );
    assert!(
        v["data"].get("source_commit").is_some(),
        "missing source_commit field: {v}"
    );
    assert!(
        v["data"].get("target").is_some(),
        "missing target field: {v}"
    );
    assert!(
        v["data"].get("target_triple").is_some(),
        "missing target_triple field: {v}"
    );
    assert_eq!(v["data"]["binary_version"], "0.0.0-dev");
    assert_eq!(v["data"]["package_version"], "0.0.0");
    assert_eq!(v["data"]["release_build"], false);
    assert_eq!(v["data"]["release_tag"], serde_json::Value::Null);
    assert_eq!(v["data"]["source_commit"], serde_json::Value::Null);
    assert_eq!(
        v["data"]["target"],
        format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
    );
    assert_eq!(v["data"]["target_triple"], serde_json::Value::Null);
    assert_eq!(v["data"]["version_status"], "dev");
    assert_eq!(v["data"]["expected_release_tag"], "v0.0.0");
    assert_eq!(v["data"]["manifest_schema_version"], 5);
    assert_eq!(v["data"]["build_receipt_schema_version"], 1);
    assert_eq!(v["data"]["install_provenance_schema_version"], 1);
}
