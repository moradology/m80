//! Ignored KVM end-to-end tests for interactive PTY behavior.
//!
//! These tests spawn the `m80` binary exactly as a user would, but attach it to
//! a host pseudo-terminal so `m80 run -it` can enter raw mode and forward live
//! input. They require a KVM-capable Linux host with Firecracker, jailer, and
//! built m80 artifacts:
//!
//! ```text
//! sudo M80_FIRECRACKER_BIN=/path/to/firecracker \
//!      M80_JAILER_BIN=/path/to/jailer \
//!      M80_JAILER_HARDEN_BIN=/path/to/m80-jailer-harden \
//!      M80_KERNEL_IMAGE=/path/to/vmlinux \
//!      M80_ROOTFS_IMAGE=/path/to/rootfs.ext4 \
//!      cargo test -p m80-cli --test e2e_tty -- --ignored
//! ```
//!
//! `M80_RUN_ROOT`, `HOME`, `M80_CGROUP_MODE`, and
//! `M80_MAX_CONCURRENT_VMS` are set by the fixture. The automated probe uses
//! `--egress none` so the terminal path is isolated from deferred outbound NAT
//! support.

use common::{append_file, run_root_entries};
mod common;
use std::fs::{self, File};
use std::io::{Read as _, Write as _};
use std::os::fd::AsFd;
use std::path::{Path, PathBuf};
use std::process::{Child, ExitStatus, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use nix::pty::openpty;
use nix::sys::termios::{self, InputFlags, LocalFlags, OutputFlags};
use tempfile::TempDir;

const READY_MARKER: &[u8] = b"M80_TTY_READY";
const INPUT_LINE: &[u8] = b"hello-from-host-pty\n";
const GOT_MARKER: &[u8] = b"M80_TTY_GOT:hello-from-host-pty";
const ANSI_HIDE_CURSOR: &[u8] = b"\x1b[?25l";
const ANSI_SHOW_CURSOR: &[u8] = b"\x1b[?25h";

struct KvmFixture {
    home: TempDir,
    run_root: TempDir,
}

struct ChildGuard {
    child: Option<Child>,
}

impl ChildGuard {
    fn new(child: Child) -> Self {
        Self { child: Some(child) }
    }

    fn wait(mut self) -> std::io::Result<ExitStatus> {
        let mut child = self.child.take().expect("child already waited");
        child.wait()
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

    fn std_m80(&self) -> std::process::Command {
        let mut cmd = std::process::Command::new(assert_cmd::cargo::cargo_bin("m80"));
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

    fn run_root(&self) -> &Path {
        self.run_root.path()
    }
}

fn run_root_parent() -> PathBuf {
    std::env::var_os("M80_E2E_RUN_ROOT_PARENT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/var/lib"))
}

#[test]
#[ignore = "requires KVM host with real Firecracker binary and m80 artifacts"]
fn interactive_tty_probe_reads_input_writes_ansi_and_preserves_exit_code() {
    let fixture = KvmFixture::new();
    let workspace = tempfile::tempdir().expect("workspace tempdir");
    fs::write(workspace.path().join("probe.txt"), "workspace-ok\n").expect("workspace write");

    let pty = openpty(None, None).expect("host pty");
    let mut master = File::from(pty.master);
    let slave = File::from(pty.slave);

    let child = fixture
        .std_m80()
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
            "--tty",
            "-i",
            "--",
            "/bin/sh",
            "-c",
            "printf '\\033[?25l'; stty size; cat probe.txt; printf 'M80_TTY_READY\\n'; IFS= read -r line; printf 'M80_TTY_GOT:%s\\n' \"$line\"; printf '\\033[?25h'; exit 13",
        ])
        .stdin(Stdio::from(slave.try_clone().expect("clone pty slave stdin")))
        .stdout(Stdio::from(slave.try_clone().expect("clone pty slave stdout")))
        .stderr(Stdio::from(slave))
        .spawn()
        .expect("spawn m80 run -it");
    let child = ChildGuard::new(child);

    let mut reader = master.try_clone().expect("clone pty master reader");
    let (chunk_tx, chunk_rx) = mpsc::channel();
    let reader_thread = std::thread::spawn(move || {
        let mut buf = [0u8; 1024];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => return,
                Ok(n) => {
                    if chunk_tx.send(buf[..n].to_vec()).is_err() {
                        return;
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => return,
            }
        }
    });

    let mut output = Vec::new();
    read_until(
        &chunk_rx,
        &mut output,
        READY_MARKER,
        Duration::from_secs(30),
        fixture.run_root(),
    );
    master.write_all(INPUT_LINE).expect("write host pty input");
    master.flush().expect("flush host pty input");
    read_until(
        &chunk_rx,
        &mut output,
        GOT_MARKER,
        Duration::from_secs(30),
        fixture.run_root(),
    );

    let status = child.wait().expect("wait m80");
    drop(master);
    let _ = reader_thread.join();

    assert_eq!(
        status.code(),
        Some(13),
        "PTY probe should return the guest exit code\n{}",
        failure_report(&output, fixture.run_root())
    );
    assert!(
        output
            .windows(ANSI_HIDE_CURSOR.len())
            .any(|w| w == ANSI_HIDE_CURSOR),
        "PTY probe should preserve ANSI/control output\n{}",
        failure_report(&output, fixture.run_root())
    );
    assert!(
        output
            .windows(ANSI_SHOW_CURSOR.len())
            .any(|w| w == ANSI_SHOW_CURSOR),
        "PTY probe should preserve final ANSI/control output\n{}",
        failure_report(&output, fixture.run_root())
    );
    let printable_output = String::from_utf8_lossy(&output)
        .replace('\r', "")
        .replace("\x1b[?25l", "")
        .replace("\x1b[?25h", "");
    assert!(
        printable_output
            .lines()
            .any(|line| line.split_whitespace().count() == 2
                && line
                    .split_whitespace()
                    .all(|part| part.parse::<u16>().is_ok_and(|n| n > 0))),
        "PTY probe should expose a terminal size from stty\n{}",
        failure_report(&output, fixture.run_root())
    );
    assert!(
        output
            .windows(b"workspace-ok".len())
            .any(|w| w == b"workspace-ok"),
        "PTY probe should see the explicit workspace\n{}",
        failure_report(&output, fixture.run_root())
    );
    fixture.assert_run_root_empty();
}

#[test]
#[ignore = "requires KVM host with real Firecracker binary and m80 artifacts"]
fn interactive_tty_enables_and_restores_host_raw_mode() {
    let fixture = KvmFixture::new();
    let pty = openpty(None, None).expect("host pty");
    let master = File::from(pty.master);
    let slave = File::from(pty.slave);
    let probe_slave = slave.try_clone().expect("clone pty slave probe");
    let original = termios::tcgetattr(probe_slave.as_fd()).expect("read original host pty mode");

    let child = fixture
        .std_m80()
        .args([
            "run",
            "--egress",
            "none",
            "--tty",
            "-i",
            "--",
            "/bin/sh",
            "-c",
            "printf 'M80_RAW_READY\\n'; sleep 5; exit 0",
        ])
        .stdin(Stdio::from(
            slave.try_clone().expect("clone pty slave stdin"),
        ))
        .stdout(Stdio::from(
            slave.try_clone().expect("clone pty slave stdout"),
        ))
        .stderr(Stdio::from(slave))
        .spawn()
        .expect("spawn m80 run -it raw-mode probe");
    let child = ChildGuard::new(child);

    let mut reader = master.try_clone().expect("clone pty master reader");
    let (chunk_tx, chunk_rx) = mpsc::channel();
    let reader_thread = std::thread::spawn(move || {
        let mut buf = [0u8; 1024];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => return,
                Ok(n) => {
                    if chunk_tx.send(buf[..n].to_vec()).is_err() {
                        return;
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => return,
            }
        }
    });

    let mut output = Vec::new();
    read_until(
        &chunk_rx,
        &mut output,
        b"M80_RAW_READY",
        Duration::from_secs(30),
        fixture.run_root(),
    );

    let raw = termios::tcgetattr(probe_slave.as_fd()).expect("read active host pty mode");
    assert_host_raw_mode(&raw, fixture.run_root(), &output);

    let status = child.wait().expect("wait m80");
    assert_eq!(
        status.code(),
        Some(0),
        "raw-mode probe should return the guest exit code\n{}",
        failure_report(&output, fixture.run_root())
    );
    let restored = termios::tcgetattr(probe_slave.as_fd()).expect("read restored host pty mode");
    assert_eq!(
        restored.input_flags, original.input_flags,
        "host pty input flags should be restored after m80 exits"
    );
    assert_eq!(
        restored.output_flags, original.output_flags,
        "host pty output flags should be restored after m80 exits"
    );
    assert_eq!(
        restored.local_flags, original.local_flags,
        "host pty local flags should be restored after m80 exits"
    );
    drop(probe_slave);
    drop(master);
    let _ = reader_thread.join();
    fixture.assert_run_root_empty();
}

fn read_until(
    chunk_rx: &mpsc::Receiver<Vec<u8>>,
    output: &mut Vec<u8>,
    needle: &[u8],
    timeout: Duration,
    run_root: &Path,
) {
    let deadline = Instant::now() + timeout;
    while !output.windows(needle.len()).any(|w| w == needle) {
        let now = Instant::now();
        assert!(
            now < deadline,
            "timed out waiting for marker {:?}\n{}",
            String::from_utf8_lossy(needle),
            failure_report(output, run_root)
        );
        let remaining = deadline.saturating_duration_since(now);
        match chunk_rx.recv_timeout(remaining.min(Duration::from_millis(250))) {
            Ok(chunk) => output.extend_from_slice(&chunk),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => panic!(
                "PTY output closed before marker {:?}\n{}",
                String::from_utf8_lossy(needle),
                failure_report(output, run_root)
            ),
        }
    }
}

fn assert_host_raw_mode(termios: &termios::Termios, run_root: &Path, output: &[u8]) {
    assert!(
        !termios.local_flags.intersects(
            LocalFlags::ECHO | LocalFlags::ICANON | LocalFlags::ISIG | LocalFlags::IEXTEN
        ),
        "host pty local flags should be raw while m80 run -it is active\n{}",
        failure_report(output, run_root)
    );
    assert!(
        !termios.input_flags.intersects(
            InputFlags::BRKINT
                | InputFlags::ICRNL
                | InputFlags::INPCK
                | InputFlags::ISTRIP
                | InputFlags::IXON
        ),
        "host pty input flags should be raw while m80 run -it is active\n{}",
        failure_report(output, run_root)
    );
    assert!(
        !termios.output_flags.contains(OutputFlags::OPOST),
        "host pty output flags should be raw while m80 run -it is active\n{}",
        failure_report(output, run_root)
    );
}

fn failure_report(output: &[u8], run_root: &Path) -> String {
    format!(
        "pty_output={:?}\nrequest_id/run-dir/console evidence lives under:\n{}",
        String::from_utf8_lossy(output),
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
        }
    }
    out
}
