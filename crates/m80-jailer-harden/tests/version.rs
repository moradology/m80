use std::process::Command;

#[test]
fn jailer_harden_version_prints_package_version_without_hardening() {
    let output = Command::new(env!("CARGO_BIN_EXE_m80-jailer-harden"))
        .arg("--version")
        .output()
        .unwrap();

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!("m80-jailer-harden {}\n", env!("CARGO_PKG_VERSION"))
    );
    assert!(output.stderr.is_empty());
}
