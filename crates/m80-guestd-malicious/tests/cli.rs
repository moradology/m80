#[test]
fn list_attacks_includes_noop() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_m80-guestd-malicious"))
        .arg("--list-attacks")
        .output()
        .expect("run m80-guestd-malicious --list-attacks");
    assert!(
        out.status.success(),
        "status={:?} stderr={}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.lines().any(|line| line == "noop"), "{stdout}");
    assert!(
        stdout.lines().any(|line| line == "oversized_length"),
        "{stdout}"
    );
}

#[test]
fn check_config_uses_attack_flag() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_m80-guestd-malicious"))
        .args(["--attack", "noop", "--check-config"])
        .output()
        .expect("run m80-guestd-malicious --check-config");
    assert!(
        out.status.success(),
        "status={:?} stderr={}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "attack=noop");
}

#[test]
fn check_config_uses_attack_env() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_m80-guestd-malicious"))
        .arg("--check-config")
        .env("M80_MALICIOUS_ATTACK", "noop")
        .output()
        .expect("run m80-guestd-malicious with env attack");
    assert!(
        out.status.success(),
        "status={:?} stderr={}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "attack=noop");
}

#[test]
fn missing_attack_fails_closed() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_m80-guestd-malicious"))
        .arg("--check-config")
        .env_remove("M80_MALICIOUS_ATTACK")
        .output()
        .expect("run m80-guestd-malicious without attack");
    assert!(!out.status.success(), "missing attack must fail");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("missing malicious guestd attack"),
        "stderr={stderr}"
    );
}
