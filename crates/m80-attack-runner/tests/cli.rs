#![cfg(feature = "malicious-artifact")]

use assert_cmd::Command;

#[test]
fn missing_attack_name_lists_known_attacks() {
    let output = Command::cargo_bin("m80-attack-runner")
        .expect("m80-attack-runner binary should be built for CLI tests")
        .output()
        .expect("m80-attack-runner should execute without spawn failure");

    assert_eq!(output.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("known attacks:"),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn unknown_attack_exits_blocked() {
    let output = Command::cargo_bin("m80-attack-runner")
        .expect("m80-attack-runner binary should be built for CLI tests")
        .arg("missing_attack")
        .output()
        .expect("m80-attack-runner should execute without spawn failure");

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("unknown attack missing_attack"),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn echo_zero_negative_control_exits_success() {
    Command::cargo_bin("m80-attack-runner")
        .expect("m80-attack-runner binary should be built for CLI tests")
        .arg("echo_zero")
        .assert()
        .success();
}

#[test]
fn api_sock_form_selects_attack_for_jailer_harness() {
    Command::cargo_bin("m80-attack-runner")
        .expect("m80-attack-runner binary should be built for CLI tests")
        .args(["--api-sock", "echo_zero"])
        .assert()
        .success();
}
