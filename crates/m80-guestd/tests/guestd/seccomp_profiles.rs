use std::process::Command;

fn guestd_bin() -> &'static str {
    env!("CARGO_BIN_EXE_m80-guestd")
}

fn seccomp_probe(profile: &str, action: &str) -> std::process::Output {
    Command::new(guestd_bin())
        .arg("--m80-seccomp-probe")
        .arg(profile)
        .arg(action)
        .output()
        .expect("spawn m80-guestd seccomp probe")
}

fn assert_probe_ok(profile: &str, action: &str, expected_stdout: &str) {
    let output = seccomp_probe(profile, action);
    assert!(
        output.status.success(),
        "probe {profile} {action} failed: status={:?} stdout={} stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        expected_stdout
    );
}

#[test]
fn daemon_seccomp_probe_enters_filter_mode() {
    assert_probe_ok("daemon", "mode", "seccomp=2");
}

#[test]
fn workload_seccomp_probe_enters_filter_mode() {
    assert_probe_ok("workload", "mode", "seccomp=2");
}

#[test]
fn workload_seccomp_probe_blocks_denied_namespace_syscall() {
    assert_probe_ok("workload", "deny-unshare", "unshare=blocked");
}

#[test]
fn workload_seccomp_probe_preserves_ordinary_exec() {
    assert_probe_ok("workload", "ordinary-exec", "ordinary_exec=ok");
}
