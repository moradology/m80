//! Smoke test: `m80 version` exits 0 and prints something meaningful.


mod common;

use common::m80;

#[test]
fn version_exits_zero() {
    m80().arg("version").assert().success();
}

#[test]
fn version_prints_binary_version() {
    let output = m80().arg("version").output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Must mention the crate version (0.0.0 in this workspace).
    assert!(
        stdout.contains("0.0.0") || stdout.contains("m80"),
        "version output should mention binary version: {stdout}"
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
}
