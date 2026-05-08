use assert_cmd::Command;

#[test]
fn missing_attack_name_lists_known_attacks() {
    let output = Command::cargo_bin("m80-attack-runner")
        .unwrap()
        .output()
        .unwrap();

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
        .unwrap()
        .arg("missing_attack")
        .output()
        .unwrap();

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
        .unwrap()
        .arg("echo_zero")
        .assert()
        .success();
}

#[test]
fn api_sock_form_selects_attack_for_jailer_harness() {
    Command::cargo_bin("m80-attack-runner")
        .unwrap()
        .args(["--api-sock", "echo_zero"])
        .assert()
        .success();
}
