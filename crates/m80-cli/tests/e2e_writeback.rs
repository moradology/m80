//! Ignored real-KVM tests for `m80 run --writeback` user-visible effects.

use common::{append_file, run_root_entries};
mod common;

use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use tempfile::TempDir;

struct KvmFixture {
    home: TempDir,
    run_root: TempDir,
}

impl KvmFixture {
    fn new() -> Self {
        Self {
            home: tempfile::tempdir().expect("home tempdir"),
            run_root: tempfile::Builder::new()
                .prefix("m")
                .tempdir_in(run_root_parent())
                .expect("run-root tempdir"),
        }
    }

    fn m80(&self) -> Command {
        let mut cmd = Command::cargo_bin("m80").expect("m80 binary");
        cmd.env("HOME", self.home.path())
            .env("M80_RUN_ROOT", self.run_root.path())
            .env("M80_CGROUP_MODE", "disabled")
            .env("M80_MAX_CONCURRENT_VMS", "1");
        cmd
    }

    fn assert_run_root_empty(&self) {
        let entries = run_root_entries(self.run_root.path())
            .into_iter()
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_none_or(|name| !name.starts_with(".rootfs-overlay-template-v1-"))
            })
            .collect::<Vec<_>>();
        assert!(
            entries.is_empty(),
            "m80 run must stop/delete sandbox state; leftover entries: {entries:?}\n{}",
            dump_run_root(self.run_root.path())
        );
    }
}

fn run_root_parent() -> PathBuf {
    std::env::var_os("M80_E2E_RUN_ROOT_PARENT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/var/lib"))
}

#[test]
#[ignore = "requires KVM host with real Firecracker binary and m80 artifacts"]
fn never_with_exit_0_preserves_host() {
    run_writeback_case("never", 0, false);
}

#[test]
#[ignore = "requires KVM host with real Firecracker binary and m80 artifacts"]
fn never_with_exit_nonzero_preserves_host() {
    run_writeback_case("never", 7, false);
}

#[test]
#[ignore = "requires KVM host with real Firecracker binary and m80 artifacts"]
fn on_success_with_exit_0_extracts() {
    run_writeback_case("on-success", 0, true);
}

#[test]
#[ignore = "requires KVM host with real Firecracker binary and m80 artifacts"]
fn on_success_with_failed_exit_does_not_extract() {
    run_writeback_case("on-success", 7, false);
}

#[test]
#[ignore = "requires KVM host with real Firecracker binary and m80 artifacts"]
fn always_with_exit_0_extracts() {
    run_writeback_case("always", 0, true);
}

#[test]
#[ignore = "requires KVM host with real Firecracker binary and m80 artifacts"]
fn always_with_failed_exit_still_extracts() {
    run_writeback_case("always", 7, true);
}

fn run_writeback_case(writeback: &str, guest_exit: i32, expect_extracted: bool) {
    let fixture = KvmFixture::new();
    let workspace = tempfile::tempdir().expect("workspace tempdir");
    fs::write(workspace.path().join("seed.txt"), b"original\n").expect("seed write");

    let output = fixture
        .m80()
        .args([
            "run",
            "--egress",
            "none",
            "--workspace",
            workspace.path().to_str().expect("utf8 workspace"),
            "--cwd",
            "/workspace",
            "--scratch-size",
            "67108864",
            "--writeback",
            writeback,
            "--",
            "/bin/sh",
            "-c",
            &format!("printf changed > added.txt; exit {guest_exit}"),
        ])
        .output()
        .expect("m80 run");

    assert_eq!(
        output.status.code(),
        Some(guest_exit),
        "m80 run must preserve the guest exit code\n{}",
        failure_report(&output, fixture.run_root.path())
    );
    assert_eq!(output.stdout, b"");
    assert_eq!(
        output.stderr,
        b"",
        "writeback policy must not require wrapper diagnostics\n{}",
        failure_report(&output, fixture.run_root.path())
    );
    assert_eq!(
        fs::read(workspace.path().join("seed.txt")).expect("read seed"),
        b"original\n",
        "writeback must preserve pre-existing host workspace files"
    );

    let added = workspace.path().join("added.txt");
    if expect_extracted {
        assert_eq!(
            fs::read(&added).expect("read extracted file"),
            b"changed",
            "{writeback} with exit {guest_exit} must extract guest workspace changes"
        );
    } else {
        assert!(
            !added.exists(),
            "{writeback} with exit {guest_exit} must leave guest workspace changes unextracted"
        );
    }

    fixture.assert_run_root_empty();
}

fn failure_report(output: &std::process::Output, run_root: &Path) -> String {
    format!(
        "status={:?}\nstdout={:?}\nstderr={:?}\n{}",
        output.status.code(),
        output.stdout,
        output.stderr,
        dump_run_root(run_root)
    )
}

fn dump_run_root(run_root: &Path) -> String {
    let mut out = format!("=== run root: {} ===\n", run_root.display());
    let entries = match fs::read_dir(run_root) {
        Ok(entries) => entries,
        Err(e) => {
            out.push_str(&format!("read_dir failed: {e}\n"));
            return out;
        }
    };

    let mut dirs = entries
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()
        .unwrap_or_else(|e| {
            out.push_str(&format!("read_dir entry failed: {e}\n"));
            Vec::new()
        });
    dirs.sort();

    if dirs.is_empty() {
        out.push_str("(empty)\n");
        return out;
    }

    for dir in dirs {
        out.push_str(&format!("--- {} ---\n", dir.display()));
        if dir.is_dir() {
            append_file(&mut out, &dir, "state.json");
            append_file(&mut out, &dir, "console.log");
            append_file(&mut out, &dir, "diagnostics.jsonl");
        }
    }
    out
}
