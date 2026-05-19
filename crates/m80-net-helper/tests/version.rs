use std::process::Command;

#[test]
fn net_helper_version_prints_package_version_without_protocol_input() {
    let output = Command::new(env!("CARGO_BIN_EXE_m80-net-helper"))
        .arg("--version")
        .output()
        .unwrap();

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!("m80-net-helper {}\n", env!("CARGO_PKG_VERSION"))
    );
    assert!(output.stderr.is_empty());
}
