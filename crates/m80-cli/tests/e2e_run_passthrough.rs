//! Ignored KVM end-to-end tests for `m80 run` passthrough behavior.
//!
//! These tests spawn the `m80` binary exactly as a user would. They require a
//! KVM-capable Linux host with Firecracker, jailer, and built m80 artifacts:
//!
//! ```text
//! sudo M80_FIRECRACKER_BIN=/path/to/firecracker \
//!      M80_JAILER_BIN=/path/to/jailer \
//!      M80_KERNEL_IMAGE=/path/to/vmlinux \
//!      M80_ROOTFS_IMAGE=/path/to/rootfs.ext4 \
//!      cargo test -p m80-cli --test e2e_run_passthrough -- --ignored
//! ```
//!
//! `M80_RUN_ROOT`, `HOME`, `M80_CGROUP_MODE`, and
//! `M80_MAX_CONCURRENT_VMS` are set by the fixture.


use common::{append_file, run_root_entries};
mod common;
use std::fs;
use std::io::{BufRead as _, Read as _};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::mpsc;
use std::time::Duration;

use assert_cmd::Command;
use nix::sys::signal::{kill, Signal};
use nix::unistd::Pid;
use tempfile::TempDir;

struct KvmFixture {
    home: TempDir,
    run_root: TempDir,
}

impl KvmFixture {
    fn new() -> Self {
        Self {
            home: tempfile::tempdir().expect("home tempdir"),
            run_root: tempfile::tempdir().expect("run-root tempdir"),
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

    fn std_m80(&self) -> std::process::Command {
        let mut cmd = std::process::Command::new(assert_cmd::cargo::cargo_bin("m80"));
        cmd.env("HOME", self.home.path())
            .env("M80_RUN_ROOT", self.run_root.path())
            .env("M80_CGROUP_MODE", "disabled")
            .env("M80_MAX_CONCURRENT_VMS", "1");
        cmd
    }

    fn assert_run_root_empty(&self) {
        let entries = run_root_entries(self.run_root.path());
        assert!(
            entries.is_empty(),
            "m80 run must stop/delete sandbox state; leftover entries: {entries:?}\n{}",
            dump_run_root(self.run_root.path())
        );
    }

    fn run_root(&self) -> &Path {
        self.run_root.path()
    }
}

#[test]
#[ignore = "requires KVM host with real Firecracker binary and m80 artifacts"]
fn run_passthrough_separates_stdio_and_exits_zero() {
    let fixture = KvmFixture::new();
    let workspace = tempfile::tempdir().expect("workspace tempdir");
    fs::write(workspace.path().join("marker.txt"), "workspace-visible\n").expect("marker write");

    let output = fixture
        .m80()
        .args([
            "run",
            "--egress",
            "none",
            "--workspace",
            workspace
                .path()
                .as_os_str()
                .to_str()
                .expect("utf8 workspace"),
            "--cwd",
            "/workspace",
            "--",
            "/bin/sh",
            "-c",
            "cat marker.txt; printf guest-stderr >&2",
        ])
        .output()
        .expect("m80 run");

    assert_eq!(
        output.status.code(),
        Some(0),
        "m80 run should return guest exit code 0\n{}",
        failure_report(&output, fixture.run_root())
    );
    assert_eq!(
        output.stdout,
        b"workspace-visible\n",
        "guest stdout must be byte-for-byte host stdout\n{}",
        failure_report(&output, fixture.run_root())
    );
    assert_eq!(
        output.stderr,
        b"guest-stderr",
        "guest stderr must be byte-for-byte host stderr\n{}",
        failure_report(&output, fixture.run_root())
    );
    fixture.assert_run_root_empty();
}

#[test]
#[ignore = "requires KVM host with real Firecracker binary and m80 artifacts"]
fn failing_guest_exit_passthrough_still_deletes_sandbox() {
    let fixture = KvmFixture::new();

    let output = fixture
        .m80()
        .args([
            "run",
            "--egress",
            "none",
            "--",
            "/bin/sh",
            "-c",
            "printf fail-out; printf fail-err >&2; exit 17",
        ])
        .output()
        .expect("m80 run");

    assert_eq!(
        output.status.code(),
        Some(17),
        "m80 run should return the nonzero guest exit code\n{}",
        failure_report(&output, fixture.run_root())
    );
    assert_eq!(
        output.stdout,
        b"fail-out",
        "guest stdout must not be contaminated by wrapper diagnostics\n{}",
        failure_report(&output, fixture.run_root())
    );
    assert_eq!(
        output.stderr,
        b"fail-err",
        "guest stderr must remain on host stderr for nonzero exits\n{}",
        failure_report(&output, fixture.run_root())
    );
    fixture.assert_run_root_empty();
}

#[test]
#[ignore = "requires KVM host with real Firecracker binary and m80 artifacts"]
fn stdout_chunk_arrives_before_guest_process_exits() {
    let fixture = KvmFixture::new();
    let mut child = fixture
        .std_m80()
        .args([
            "run",
            "--egress",
            "none",
            "--",
            "/bin/sh",
            "-c",
            "printf early; sleep 5; printf late",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn m80 run");

    let mut stdout = child.stdout.take().expect("stdout pipe");
    let mut stderr = child.stderr.take().expect("stderr pipe");
    let (first_tx, first_rx) = mpsc::channel();
    let stdout_reader = std::thread::spawn(move || {
        let mut first = vec![0; 5];
        stdout.read_exact(&mut first)?;
        let _ = first_tx.send(first);
        let mut rest = Vec::new();
        stdout.read_to_end(&mut rest)?;
        Ok::<_, std::io::Error>(rest)
    });
    let stderr_reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stderr.read_to_end(&mut bytes)?;
        Ok::<_, std::io::Error>(bytes)
    });

    let first = match first_rx.recv_timeout(Duration::from_secs(2)) {
        Ok(first) => first,
        Err(e) => {
            let _ = child.kill();
            let _ = child.wait();
            panic!(
                "expected first stdout chunk before child exit: {e}\n{}",
                dump_run_root(fixture.run_root())
            );
        }
    };
    assert_eq!(first, b"early");
    assert!(
        child.try_wait().expect("try_wait").is_none(),
        "guest should still be sleeping when the first stdout chunk is visible"
    );

    let status = child.wait().expect("wait m80");
    let rest = stdout_reader
        .join()
        .expect("stdout reader")
        .expect("read stdout rest");
    let stderr = stderr_reader
        .join()
        .expect("stderr reader")
        .expect("read stderr");

    assert_eq!(
        status.code(),
        Some(0),
        "m80 run should complete after delayed output\nstderr={stderr:?}\n{}",
        dump_run_root(fixture.run_root())
    );
    assert_eq!(rest, b"late");
    assert!(
        stderr.is_empty(),
        "wrapper diagnostics must not be needed on stderr for a successful stream: {stderr:?}"
    );
    fixture.assert_run_root_empty();
}

#[test]
#[ignore = "requires KVM host with real Firecracker binary and m80 artifacts"]
fn pipe_streaming_stdout_is_not_capped_at_one_mib() {
    let fixture = KvmFixture::new();

    let output = fixture
        .m80()
        .args([
            "run",
            "--egress",
            "none",
            "--",
            "/bin/sh",
            "-c",
            "dd if=/dev/zero bs=1048576 count=2 2>/dev/null | tr '\\000' x",
        ])
        .output()
        .expect("m80 run");

    assert_eq!(
        output.status.code(),
        Some(0),
        "large-output probe should exit 0\n{}",
        failure_report(&output, fixture.run_root())
    );
    assert!(
        output.stdout.len() > 1_048_576,
        "pipe-mode streaming must not be capped at the buffered exec limit; len={}\n{}",
        output.stdout.len(),
        failure_report(&output, fixture.run_root())
    );
    assert!(
        output.stderr.is_empty(),
        "large-output probe should keep wrapper diagnostics off stderr: {:?}",
        output.stderr
    );
    fixture.assert_run_root_empty();
}

#[test]
#[ignore = "requires KVM host with real Firecracker binary and m80 artifacts"]
fn run_tty_smoke_uses_guest_terminal_and_preserves_exit_code() {
    let fixture = KvmFixture::new();

    let output = fixture
        .m80()
        .args([
            "run",
            "--egress",
            "none",
            "--tty",
            "--",
            "/bin/sh",
            "-c",
            "stty size; printf pty-ok; exit 9",
        ])
        .output()
        .expect("m80 run --tty");

    assert_eq!(
        output.status.code(),
        Some(9),
        "PTY run should return the guest exit code\n{}",
        failure_report(&output, fixture.run_root())
    );
    assert!(
        output.stderr.is_empty(),
        "successful PTY execution should not need wrapper diagnostics on stderr: {:?}",
        output.stderr
    );
    let stdout = String::from_utf8_lossy(&output.stdout).replace('\r', "");
    assert!(
        stdout.contains("pty-ok"),
        "PTY stdout should contain merged terminal bytes\n{}",
        failure_report(&output, fixture.run_root())
    );
    let size_line = stdout.lines().next().unwrap_or_default();
    let dims = size_line
        .split_whitespace()
        .map(|part| part.parse::<u16>())
        .collect::<Result<Vec<_>, _>>()
        .unwrap_or_else(|e| panic!("expected stty size line, got {size_line:?}: {e}"));
    assert_eq!(dims.len(), 2, "expected `rows cols`, got {size_line:?}");
    assert!(dims[0] > 0);
    assert!(dims[1] > 0);
    fixture.assert_run_root_empty();
}

#[test]
#[ignore = "requires KVM host with real Firecracker binary and m80 artifacts"]
fn sigterm_cancels_guest_child_and_deletes_sandbox() {
    let fixture = KvmFixture::new();
    let mut child = fixture
        .std_m80()
        .args([
            "run",
            "--egress",
            "none",
            "--",
            "/bin/sh",
            "-c",
            "printf 'ready\\n'; sleep 60 & wait",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn m80 run");

    let stdout = child.stdout.take().expect("stdout pipe");
    let mut stdout = std::io::BufReader::new(stdout);
    let mut stderr = child.stderr.take().expect("stderr pipe");
    let mut ready = String::new();
    stdout.read_line(&mut ready).expect("read readiness line");
    assert_eq!(ready, "ready\n");

    kill(Pid::from_raw(child.id() as i32), Signal::SIGTERM).expect("send SIGTERM to m80");

    let status = child.wait().expect("wait m80");
    let mut stdout_rest = Vec::new();
    let mut stderr_bytes = Vec::new();
    stdout
        .read_to_end(&mut stdout_rest)
        .expect("read stdout rest");
    stderr
        .read_to_end(&mut stderr_bytes)
        .expect("read stderr rest");

    assert_eq!(
        status.code(),
        Some(143),
        "SIGTERM should map to conventional process exit 143\nstderr={stderr_bytes:?}\n{}",
        dump_run_root(fixture.run_root())
    );
    assert!(
        stdout_rest.is_empty(),
        "guest should not continue to produce stdout after cancellation: {stdout_rest:?}"
    );
    assert!(
        stderr_bytes.is_empty(),
        "signal cancellation should not add wrapper diagnostics to stderr: {stderr_bytes:?}"
    );
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


