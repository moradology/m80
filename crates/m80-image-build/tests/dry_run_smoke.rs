//! Smoke test: `run --dry-run` emits expected step lines to stderr and
//! creates no files.
//!
//! Uses `assert_cmd` to invoke the binary; no root needed.

use std::path::{Path, PathBuf};

use assert_cmd::Command;
use tempfile::TempDir;

/// Write a valid `m80-image-build.toml` into `dir` and return its path.
fn write_fixture_config(dir: &TempDir, rootfs: &str) -> PathBuf {
    let config_path = dir.path().join("m80-image-build.toml");
    let guestd_bin = dir.path().join("fake-guestd");
    // The binary path just needs to be a plausible string for dry-run.
    std::fs::write(&guestd_bin, b"fake").unwrap();
    let out_dir = dir.path().join("out");
    let toml = format!(
        r#"
[kernel]
version = "v1.15.1"
artifact_track = "v1.15"
arch = "x86_64"

[rootfs]
{rootfs}

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

fn write_ubuntu_fixture_config(dir: &TempDir) -> PathBuf {
    write_fixture_config(dir, r#"size = "1GiB""#)
}

fn write_minimal_fixture_config(dir: &TempDir) -> PathBuf {
    write_fixture_config(
        dir,
        r#"kind = "minimal"
size = "256MiB""#,
    )
}

fn write_minimal_erofs_fixture_config(dir: &TempDir) -> PathBuf {
    write_fixture_config(
        dir,
        r#"kind = "minimal-erofs"
size = "256MiB""#,
    )
}

fn dry_run_steps(config_path: &Path) -> (std::process::ExitStatus, String) {
    let mut cmd = Command::cargo_bin("m80-image-build").unwrap();
    cmd.args([
        "run",
        "--config",
        config_path.to_str().unwrap(),
        "--dry-run",
    ]);
    let output = cmd.output().unwrap();
    (
        output.status,
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

fn observed_step_numbers(stderr: &str) -> Vec<u32> {
    stderr
        .lines()
        .filter_map(|line| {
            let prefix = line.split('.').next()?.trim();
            prefix.parse::<u32>().ok()
        })
        .collect()
}

#[test]
fn dry_run_prints_steps_to_stderr_and_creates_no_output_files() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = write_ubuntu_fixture_config(&dir);
    let out_dir = dir.path().join("out");

    let (status, stderr) = dry_run_steps(&config_path);

    assert!(
        status.success(),
        "dry-run should exit 0; stderr: {}",
        stderr
    );

    // Capture each "<n>." prefix at the start of a line; assert the sequence
    // is exactly 1..=11 in order. Using `contains` would pass even if labels
    // were duplicated or out of order.
    let observed = observed_step_numbers(&stderr);
    assert_eq!(
        observed,
        (1..=11).collect::<Vec<_>>(),
        "expected step numbers 1..=11 in order; got {observed:?}\nstderr:\n{stderr}"
    );
    assert!(
        stderr.contains("9. Compute sha256 of 4 artifacts"),
        "Ubuntu dry-run must name the source-rootfs-inclusive sha256 step; stderr:\n{stderr}"
    );

    // The output directory must not have been created.
    assert!(
        !out_dir.exists(),
        "dry-run must not create output directory"
    );
}

#[test]
fn minimal_dry_run_prints_release_artifact_steps_and_creates_no_output_files() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = write_minimal_fixture_config(&dir);
    let out_dir = dir.path().join("out");

    let (status, stderr) = dry_run_steps(&config_path);

    assert!(
        status.success(),
        "minimal dry-run should exit 0; stderr: {stderr}"
    );

    let observed = observed_step_numbers(&stderr);
    assert_eq!(
        observed,
        (1..=11).collect::<Vec<_>>(),
        "expected minimal step numbers 1..=11 in order; got {observed:?}\nstderr:\n{stderr}"
    );
    assert!(
        stderr.contains("Compute sha256 of 3 artifacts"),
        "minimal release dry-run should describe the reduced artifact set; stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("firecracker-ci/v1.15/x86_64/vmlinux-5.10.245"),
        "minimal release dry-run must use the Firecracker CI artifact track, not the exact version pin; stderr:\n{stderr}"
    );
    assert!(
        !out_dir.exists(),
        "minimal dry-run must not create output directory"
    );
}

#[test]
fn minimal_erofs_dry_run_prints_erofs_steps_and_creates_no_output_files() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = write_minimal_erofs_fixture_config(&dir);
    let out_dir = dir.path().join("out");

    let (status, stderr) = dry_run_steps(&config_path);

    assert!(
        status.success(),
        "minimal-erofs dry-run should exit 0; stderr: {stderr}"
    );

    let observed = observed_step_numbers(&stderr);
    assert_eq!(
        observed,
        (1..=9).collect::<Vec<_>>(),
        "expected minimal-erofs step numbers 1..=9 in order; got {observed:?}\nstderr:\n{stderr}"
    );
    assert!(
        stderr.contains("mkfs.erofs -zlz4hc"),
        "minimal-erofs dry-run should describe the erofs builder; stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("output.erofs"),
        "minimal-erofs dry-run should target output.erofs; stderr:\n{stderr}"
    );
    assert!(
        !out_dir.exists(),
        "minimal-erofs dry-run must not create output directory"
    );
}

#[test]
fn dry_run_step_output_is_deterministic() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = write_ubuntu_fixture_config(&dir);

    let run = || -> String {
        let (_status, stderr) = dry_run_steps(&config_path);
        stderr
    };

    let first = run();
    let second = run();
    assert_eq!(first, second, "dry-run output must be deterministic");
}
