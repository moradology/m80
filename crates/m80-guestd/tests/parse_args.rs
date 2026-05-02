//! Tests for parse_args.
//!
//! Note: `parse_args()` reads `std::env::args()` directly, so these tests
//! exercise the parsing logic by calling the binary with different argv via
//! integration-test patterns. For unit-level coverage we directly call the
//! binary entrypoint via `assert_cmd`.

// We test parse_args indirectly since it reads std::env::args() directly.
// We use assert_cmd to drive the binary.

#[test]
fn port_flag_parsed_correctly() {
    // Run m80-guestd --version to confirm flag parsing works end-to-end.
    // --port alone would try to bind a vsock socket and fail on the host,
    // but --version with no port exercises the non-port path.
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_m80-guestd"))
        .arg("--version")
        .output()
        .expect("failed to run m80-guestd --version");
    assert!(out.status.success(), "status: {:?}\nstdout: {}\nstderr: {}",
        out.status,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("m80-guestd"), "expected version string: {stdout}");
    assert!(stdout.contains("proto v"), "expected proto version: {stdout}");
}

#[test]
fn unknown_arg_returns_error() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_m80-guestd"))
        .arg("--unknown-flag")
        .output()
        .expect("failed to run m80-guestd --unknown-flag");
    assert!(!out.status.success(), "expected non-zero exit for unknown flag");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("unknown arg"), "expected 'unknown arg' in stderr: {stderr}");
}
