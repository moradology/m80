//! Smoke test for `m80 quickstart --no-run` with a local release-like tarball.

use std::process::Command as StdCommand;

use assert_cmd::Command;
use serde_json::Value;

fn m80() -> Command {
    Command::cargo_bin("m80").unwrap()
}

fn run_checked(cmd: &mut StdCommand, label: &str) {
    let output = cmd.output().unwrap();
    assert!(
        output.status.success(),
        "{label} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn output_checked(cmd: &mut StdCommand, label: &str) -> std::process::Output {
    let output = cmd.output().unwrap();
    assert!(
        output.status.success(),
        "{label} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn write_release_tarball(dir: &tempfile::TempDir) -> std::path::PathBuf {
    let src = dir.path().join("src");
    std::fs::create_dir(&src).unwrap();
    std::fs::write(src.join("vmlinux"), b"kernel").unwrap();
    std::fs::write(src.join("output.ext4"), b"rootfs").unwrap();
    std::fs::write(
        src.join("output.ext4.manifest.json"),
        b"{\"schema_version\":1}",
    )
    .unwrap();
    std::fs::write(src.join("m80-guestd"), b"guestd").unwrap();

    let sums = output_checked(
        StdCommand::new("sha256sum")
            .args([
                "vmlinux",
                "output.ext4",
                "output.ext4.manifest.json",
                "m80-guestd",
            ])
            .current_dir(&src),
        "sha256sum",
    );
    std::fs::write(src.join("SHA256SUMS"), sums.stdout).unwrap();

    let tarball = dir.path().join("m80-artifacts.tar.gz");
    run_checked(
        StdCommand::new("tar")
            .arg("-czf")
            .arg(&tarball)
            .arg("-C")
            .arg(&src)
            .arg("."),
        "tar",
    );
    tarball
}

#[test]
fn quickstart_no_run_installs_verified_artifacts() {
    let dir = tempfile::tempdir().unwrap();
    let tarball = write_release_tarball(&dir);
    let dst = dir.path().join("dst");
    let run_root = dir.path().join("run");

    m80()
        .args([
            "quickstart",
            "--artifact-url",
            &format!("file://{}", tarball.display()),
            "--artifact-dir",
            dst.to_str().unwrap(),
            "--run-root",
            run_root.to_str().unwrap(),
            "--no-run",
        ])
        .assert()
        .success();

    for artifact in [
        "vmlinux",
        "output.ext4",
        "output.ext4.manifest.json",
        "m80-guestd",
    ] {
        assert!(
            dst.join(artifact).is_file(),
            "quickstart should install {artifact}"
        );
    }
    assert!(run_root.is_dir(), "quickstart should create run-root");
}

#[test]
fn quickstart_json_no_run_keeps_stdout_machine_readable() {
    let dir = tempfile::tempdir().unwrap();
    let tarball = write_release_tarball(&dir);
    let dst = dir.path().join("json-dst");
    let run_root = dir.path().join("json-run");

    let output = m80()
        .args([
            "--json",
            "quickstart",
            "--artifact-url",
            &format!("file://{}", tarball.display()),
            "--artifact-dir",
            dst.to_str().unwrap(),
            "--run-root",
            run_root.to_str().unwrap(),
            "--no-run",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "quickstart --json failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["version"], 1);
    assert_eq!(value["data"]["ran_probe"], false);
    assert_eq!(
        value["data"]["artifact_dir"].as_str(),
        Some(dst.to_str().unwrap())
    );
}

#[test]
fn quickstart_json_requires_no_run_to_keep_stdout_machine_readable() {
    let output = m80()
        .args([
            "--json",
            "quickstart",
            "--artifact-url",
            "file:///tmp/m80-artifacts.tar.gz",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        output.stdout.is_empty(),
        "JSON quickstart config error should not write stdout"
    );

    let value: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(value["version"], 1);
    assert_eq!(value["data"]["variant"], "Config");
    assert!(
        value["data"]["detail"]
            .as_str()
            .unwrap()
            .contains("--json requires --no-run"),
        "unexpected error payload: {value}"
    );
}
