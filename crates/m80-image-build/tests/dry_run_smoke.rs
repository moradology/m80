//! Smoke test: `run --dry-run` emits expected step lines to stderr and
//! creates no files.
//!
//! Uses `assert_cmd` to invoke the binary; no root needed.

use std::path::PathBuf;

use assert_cmd::Command;
use tempfile::TempDir;

/// Write a minimal valid `m80-image-build.toml` into `dir` and return its path.
fn write_fixture_config(dir: &TempDir) -> PathBuf {
    let config_path = dir.path().join("m80-image-build.toml");
    let guestd_bin = dir.path().join("fake-guestd");
    // The binary path just needs to be a plausible string for dry-run.
    std::fs::write(&guestd_bin, b"fake").unwrap();
    let out_dir = dir.path().join("out");
    let toml = format!(
        r#"
[kernel]
version = "v1.15.1"
arch = "x86_64"

[rootfs]
size = "1GiB"
source = "firecracker-ci"

[guestd]
binary = "{}"

[output]
dir = "{}"
"#,
        guestd_bin.display(),
        out_dir.display(),
    );
    std::fs::write(&config_path, toml).unwrap();
    config_path
}

#[test]
fn dry_run_prints_steps_to_stderr_and_creates_no_output_files() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = write_fixture_config(&dir);
    let out_dir = dir.path().join("out");

    let mut cmd = Command::cargo_bin("m80-image-build").unwrap();
    cmd.args(["run", "--config", config_path.to_str().unwrap(), "--dry-run"]);
    let output = cmd.output().unwrap();

    assert!(
        output.status.success(),
        "dry-run should exit 0; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    // Expect all 12 step labels (the README's Pipeline section enumerates them).
    for i in 1..=12 {
        assert!(
            stderr.contains(&format!("{}.", i)),
            "step {} missing from stderr:\n{stderr}",
            i
        );
    }

    // The output directory must not have been created.
    assert!(
        !out_dir.exists(),
        "dry-run must not create output directory"
    );
}

#[test]
fn dry_run_step_output_is_deterministic() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = write_fixture_config(&dir);

    let run = || -> String {
        let mut cmd = Command::cargo_bin("m80-image-build").unwrap();
        cmd.args(["run", "--config", config_path.to_str().unwrap(), "--dry-run"]);
        let output = cmd.output().unwrap();
        String::from_utf8_lossy(&output.stderr).into_owned()
    };

    let first = run();
    let second = run();
    assert_eq!(first, second, "dry-run output must be deterministic");
}
