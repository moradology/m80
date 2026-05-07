//! Ignored KVM end-to-end tests for the foreground warm owner.
//!
//! These tests require a KVM-capable Linux host with Firecracker, jailer, and
//! built m80 artifacts:
//!
//! ```text
//! sudo M80_FIRECRACKER_BIN=/path/to/firecracker \
//!      M80_JAILER_BIN=/path/to/jailer \
//!      M80_KERNEL_IMAGE=/path/to/vmlinux \
//!      M80_ROOTFS_IMAGE=/path/to/rootfs.ext4 \
//!      cargo test -p m80-cli --test e2e_warm -- --ignored
//! ```


use common::{append_file, run_root_entries};
mod common;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

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
            run_root: tempfile::tempdir().expect("run-root tempdir"),
        }
    }

    fn m80(&self) -> Command {
        let mut cmd = Command::cargo_bin("m80").expect("m80 binary");
        cmd.env("HOME", self.home.path())
            .env("M80_RUN_ROOT", self.run_root.path())
            .env("M80_CGROUP_MODE", "disabled")
            .env("M80_MAX_CONCURRENT_VMS", "4");
        cmd
    }

    fn std_m80(&self) -> std::process::Command {
        let mut cmd = std::process::Command::new(assert_cmd::cargo::cargo_bin("m80"));
        cmd.env("HOME", self.home.path())
            .env("M80_RUN_ROOT", self.run_root.path())
            .env("M80_CGROUP_MODE", "disabled")
            .env("M80_MAX_CONCURRENT_VMS", "4");
        cmd
    }

    fn run_root(&self) -> &Path {
        self.run_root.path()
    }
}

struct ChildGuard {
    child: Option<Child>,
}

impl ChildGuard {
    fn new(child: Child) -> Self {
        Self { child: Some(child) }
    }

    fn wait(mut self) -> std::io::Result<std::process::ExitStatus> {
        let mut child = self.child.take().expect("child already waited");
        child.wait()
    }

    fn child_mut(&mut self) -> &mut Child {
        self.child.as_mut().expect("child already waited")
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[test]
#[ignore = "requires KVM host with real Firecracker binary and m80 artifacts"]
fn foreground_warm_owner_serves_run_and_drains_without_cold_fallback() {
    let fixture = KvmFixture::new();
    let owner = ChildGuard::new(
        fixture
            .std_m80()
            .args([
                "warm",
                "enable",
                "--foreground",
                "--size",
                "1",
                "--egress",
                "none",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn warm owner"),
    );

    wait_for_status(&fixture, "ready=1", Duration::from_secs(90));

    let output = fixture
        .m80()
        .args([
            "run",
            "--warm",
            "--egress",
            "none",
            "--",
            "/bin/sh",
            "-c",
            "printf warm-out; printf warm-err >&2; exit 17",
        ])
        .output()
        .expect("m80 run --warm");

    assert_eq!(
        output.status.code(),
        Some(17),
        "warm run should preserve guest exit code\n{}",
        failure_report(&output, fixture.run_root())
    );
    assert_eq!(output.stdout, b"warm-out");
    assert_eq!(output.stderr, b"warm-err");

    wait_for_status_any(&fixture, &["ready=1", "filling=1"], Duration::from_secs(90));
    wait_for_status(&fixture, "ready=1", Duration::from_secs(90));

    let mut streaming = ChildGuard::new(
        fixture
            .std_m80()
            .args([
                "run",
                "--warm",
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
            .expect("spawn warm streaming run"),
    );
    let stdout = streaming
        .child_mut()
        .stdout
        .take()
        .expect("warm streaming stdout");
    let rx = read_streaming_stdout(stdout, 5);
    let first = rx
        .recv_timeout(Duration::from_secs(10))
        .expect("warm streaming first chunk")
        .expect("read warm streaming first chunk");
    assert_eq!(first, b"early");
    assert!(
        streaming
            .child_mut()
            .try_wait()
            .expect("poll streaming child")
            .is_none(),
        "warm streaming child exited before late output"
    );
    let status = streaming.wait().expect("wait warm streaming run");
    assert_eq!(status.code(), Some(0));
    let all = rx
        .recv_timeout(Duration::from_secs(10))
        .expect("warm streaming complete")
        .expect("read warm streaming complete");
    assert_eq!(all, b"earlylate");

    wait_for_status(&fixture, "ready=1", Duration::from_secs(90));

    let drain = fixture
        .m80()
        .args(["warm", "drain"])
        .output()
        .expect("m80 warm drain");
    assert_eq!(
        drain.status.code(),
        Some(0),
        "drain should succeed\n{}",
        failure_report(&drain, fixture.run_root())
    );
    assert!(String::from_utf8_lossy(&drain.stdout).contains("draining"));

    let status = owner.wait().expect("wait warm owner");
    assert_eq!(status.code(), Some(0));
    assert_no_run_dirs_with_prefix(fixture.run_root(), "warm-slot");
}

fn wait_for_status(fixture: &KvmFixture, needle: &str, timeout: Duration) {
    wait_for_status_any(fixture, &[needle], timeout);
}

fn read_streaming_stdout(
    mut stdout: impl Read + Send + 'static,
    first_len: usize,
) -> mpsc::Receiver<std::io::Result<Vec<u8>>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut first = vec![0; first_len];
        if let Err(e) = stdout.read_exact(&mut first) {
            let _ = tx.send(Err(e));
            return;
        }
        let _ = tx.send(Ok(first.clone()));
        let mut rest = Vec::new();
        if let Err(e) = stdout.read_to_end(&mut rest) {
            let _ = tx.send(Err(e));
            return;
        }
        first.extend(rest);
        let _ = tx.send(Ok(first));
    });
    rx
}

fn wait_for_status_any(fixture: &KvmFixture, needles: &[&str], timeout: Duration) {
    let deadline = Instant::now() + timeout;
    loop {
        let output = fixture
            .m80()
            .args(["warm", "status"])
            .output()
            .expect("m80 warm status");
        let stdout = String::from_utf8_lossy(&output.stdout);
        if output.status.success() && needles.iter().any(|needle| stdout.contains(needle)) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for warm status {needles:?}\n{}",
            failure_report(&output, fixture.run_root())
        );
        std::thread::sleep(Duration::from_millis(250));
    }
}

fn assert_no_run_dirs_with_prefix(run_root: &Path, prefix: &str) {
    let leaked = run_root_entries(run_root)
        .into_iter()
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(prefix))
        })
        .collect::<Vec<_>>();
    assert!(
        leaked.is_empty(),
        "warm owner leaked run dirs: {leaked:?}\n{}",
        dump_run_root(run_root)
    );
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
            append_file(&mut out, &dir, "diagnostics.jsonl");
            append_file(&mut out, &dir, "console.log");
            append_file(&mut out, &dir, "owner.json");
        }
    }
    out
}


