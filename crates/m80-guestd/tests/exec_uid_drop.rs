//! Root-only integration check for the hidden guest exec shim.

#[test]
#[ignore = "requires root because it verifies the guest exec uid/gid/capability drop"]
fn exec_shim_runs_workload_non_root_with_no_new_privs_and_empty_caps() {
    let script = r#"
set -eu
printf 'uid=%s\n' "$(id -u)"
printf 'gid=%s\n' "$(id -g)"
awk '/^(NoNewPrivs|Seccomp|CapEff|CapPrm|CapInh|CapBnd):/ { print }' /proc/self/status
"#;

    let out = std::process::Command::new(env!("CARGO_BIN_EXE_m80-guestd"))
        .args(["--m80-exec-shim", "/bin/sh", "-c", script])
        .output()
        .expect("failed to run m80-guestd exec shim");

    assert!(
        out.status.success(),
        "status: {:?}\nstdout: {}\nstderr: {}",
        out.status,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("uid=1000\n"), "stdout:\n{stdout}");
    assert!(stdout.contains("gid=1000\n"), "stdout:\n{stdout}");
    assert!(stdout.contains("NoNewPrivs:\t1\n"), "stdout:\n{stdout}");
    assert!(stdout.contains("Seccomp:\t2\n"), "stdout:\n{stdout}");
    assert!(
        stdout.contains("CapEff:\t0000000000000000\n"),
        "stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("CapPrm:\t0000000000000000\n"),
        "stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("CapInh:\t0000000000000000\n"),
        "stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("CapBnd:\t0000000000000000\n"),
        "stdout:\n{stdout}"
    );
}
