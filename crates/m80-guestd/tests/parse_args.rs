//! parse_args reads `std::env::args()`, so we exercise it by spawning the
//! built binary with different argv.

#[test]
fn version_flag_prints_version_with_proto() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_m80-guestd"))
        .arg("--version")
        .output()
        .expect("failed to run m80-guestd --version");
    assert!(
        out.status.success(),
        "status: {:?}\nstderr: {}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let proto = m80_proto::PROTOCOL_VERSION.to_string();
    assert!(
        stdout.contains(&proto),
        "expected protocol version {proto} in: {stdout}"
    );
}

#[test]
fn unknown_arg_returns_error() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_m80-guestd"))
        .arg("--unknown-flag")
        .output()
        .expect("failed to run m80-guestd --unknown-flag");
    assert!(
        !out.status.success(),
        "expected non-zero exit for unknown flag"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("unknown arg"),
        "expected 'unknown arg' in stderr: {stderr}"
    );
}
