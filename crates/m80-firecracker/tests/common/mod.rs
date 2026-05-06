//! Shared test fixtures: fake `Manifest` + fake `Discovery` so each
//! integration test doesn't reinvent the placeholder values.
//!
//! Also provides [`RunDirDumpGuard`], a drop-guard that dumps
//! `<run_dir>/console.log` and `<run_dir>/diagnostics.jsonl` to stderr
//! when the test thread is panicking — making failures self-explaining
//! without any extra effort from the caller.

use std::io::{self, Write};
use std::path::{Path, PathBuf};

#[allow(dead_code)]
pub fn fake_manifest() -> m80_image_manifest::Manifest {
    m80_image_manifest::Manifest {
        schema_version: m80_image_manifest::SCHEMA_VERSION,
        image_kind: m80_image_manifest::ImageKind::Ubuntu,
        kernel_kind: m80_image_manifest::KernelKind::Stock,
        expected_firecracker_version: "v1.0.0".into(),
        kernel_image: PathBuf::from("/dev/null"),
        kernel_image_sha256: "0".repeat(64),
        output_rootfs_image: PathBuf::from("/dev/null"),
        output_rootfs_sha256: "0".repeat(64),
        source_rootfs_image: Some(PathBuf::from("/dev/null")),
        source_rootfs_sha256: Some("0".repeat(64)),
        daemon_binary_path: PathBuf::from("/dev/null"),
        daemon_binary_sha256: "0".repeat(64),
        service_unit_path: Some(PathBuf::from("/dev/null")),
        service_unit_sha256: Some("0".repeat(64)),
        workspace_mount_path: Some(PathBuf::from("/dev/null")),
        workspace_mount_sha256: Some("0".repeat(64)),
        boot_target: Some("multi-user.target".into()),
        guest_port: 9001,
        no_egress_reason: None,
        ready_marker: "GUESTD_READY".into(),
    }
}

#[allow(dead_code)]
pub fn fake_discovery(run_root: &Path) -> m80_preflight::Discovery {
    m80_preflight::Discovery {
        firecracker_bin: PathBuf::from("/dev/null"),
        jailer_bin: PathBuf::from("/dev/null"),
        kernel: PathBuf::from("/dev/null"),
        rootfs: PathBuf::from("/dev/null"),
        manifest: fake_manifest(),
        run_root: run_root.to_path_buf(),
        privilege: m80_preflight::PrivilegeStatus::Root,
        report: vec![],
    }
}

// ── RunDirDumpGuard ───────────────────────────────────────────────────────────

/// Drop-guard that dumps `console.log` (last N lines) and the full
/// `diagnostics.jsonl` from a VM run-dir to **stderr** when the test thread
/// is panicking.
///
/// # Usage
///
/// Bind the guard to a **named local** so it lives for the entire test body.
/// Do *not* rely on tail-expression temporaries — Rust drops them before the
/// panic propagates, which would make the dump unreachable.
///
/// ```no_run
/// # use std::path::PathBuf;
/// # // `common` is the tests/common module, not importable in a doc-test.
/// # struct RunDirDumpGuard;
/// # impl RunDirDumpGuard {
/// #     pub fn new(_: PathBuf) -> Self { RunDirDumpGuard }
/// # }
/// let run_dir = PathBuf::from("/tmp/m80-test/my-run-dir");
/// let _dump_guard = RunDirDumpGuard::new(run_dir.clone());
/// // … test body …
/// ```
///
/// A passing test produces no extra output. A panicking test prints the
/// run-dir path and the last [`Self::console_tail_lines`] lines of
/// `console.log` plus the full `diagnostics.jsonl`.
pub struct RunDirDumpGuard {
    run_dir: PathBuf,
    console_tail_lines: usize,
}

impl RunDirDumpGuard {
    /// Create a guard for `run_dir` with a 100-line console tail.
    ///
    /// The run-dir must already exist; this constructor does not create it.
    pub fn new(run_dir: impl Into<PathBuf>) -> Self {
        RunDirDumpGuard {
            run_dir: run_dir.into(),
            console_tail_lines: 100,
        }
    }

    /// Override the default 100-line console tail limit.
    pub fn with_tail_lines(mut self, n: usize) -> Self {
        self.console_tail_lines = n;
        self
    }
}

impl Drop for RunDirDumpGuard {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            return;
        }
        let stderr = io::stderr();
        let mut w = stderr.lock();
        // Swallow write errors: we are already in a panic path and Drop must
        // not propagate a new panic.
        let _ = dump_run_dir(&self.run_dir, self.console_tail_lines, &mut w);
    }
}

// ── dump_run_dir (testable core) ─────────────────────────────────────────────

/// Write the run-dir artifact dump to `w`.
///
/// Prints a header with the path, the last `console_tail_lines` lines of
/// `console.log`, and the full contents of `diagnostics.jsonl`. Missing
/// files are reported with `(not found)`; other I/O errors are reported
/// verbatim. This function never panics.
///
/// `Drop` calls this with `stderr().lock()`; unit tests call it with a
/// `Vec<u8>` to capture and assert on the output.
pub(super) fn dump_run_dir(
    run_dir: &Path,
    console_tail_lines: usize,
    w: &mut dyn Write,
) -> io::Result<()> {
    writeln!(w, "=== RunDirDump: {} ===", run_dir.display())?;

    // ── console.log ──────────────────────────────────────────────────────────
    let console_path = run_dir.join("console.log");
    match std::fs::read_to_string(&console_path) {
        Ok(contents) => {
            let lines: Vec<&str> = contents.lines().collect();
            let total = lines.len();
            if total > console_tail_lines {
                writeln!(
                    w,
                    "--- console.log (showing last {console_tail_lines} of {total} lines) ---"
                )?;
                for line in &lines[total - console_tail_lines..] {
                    writeln!(w, "{line}")?;
                }
            } else {
                writeln!(w, "--- console.log ({total} lines) ---")?;
                for line in &lines {
                    writeln!(w, "{line}")?;
                }
            }
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            writeln!(w, "console.log: (not found)")?;
        }
        Err(e) => {
            writeln!(w, "console.log: {e}")?;
        }
    }

    // ── diagnostics.jsonl ────────────────────────────────────────────────────
    let diag_path = run_dir.join("diagnostics.jsonl");
    match std::fs::read_to_string(&diag_path) {
        Ok(contents) => {
            writeln!(w, "--- diagnostics.jsonl ---")?;
            write!(w, "{contents}")?;
            // Ensure there is a trailing newline so the next output line is clean.
            if !contents.ends_with('\n') {
                writeln!(w)?;
            }
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            writeln!(w, "diagnostics.jsonl: (not found)")?;
        }
        Err(e) => {
            writeln!(w, "diagnostics.jsonl: {e}")?;
        }
    }

    writeln!(w, "=== end RunDirDump ===")?;
    Ok(())
}

// ── unit tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    // ── dump_run_dir scenarios ────────────────────────────────────────────────

    fn make_run_dir() -> TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    fn write(dir: &Path, name: &str, content: &str) {
        fs::write(dir.join(name), content).expect("write test file");
    }

    fn captured(dir: &Path, tail: usize) -> String {
        let mut buf: Vec<u8> = Vec::new();
        dump_run_dir(dir, tail, &mut buf).expect("dump_run_dir");
        String::from_utf8(buf).expect("utf8")
    }

    #[test]
    fn both_files_exist_output_contains_path_console_and_diag() {
        let tmp = make_run_dir();
        let dir = tmp.path();
        let console_content = (1..=5)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        write(dir, "console.log", &console_content);
        write(dir, "diagnostics.jsonl", "{\"a\":1}\n{\"b\":2}\n");

        let out = captured(dir, 100);

        assert!(
            out.contains(&dir.display().to_string()),
            "header should include run_dir path"
        );
        assert!(out.contains("line 1"), "should include console line 1");
        assert!(out.contains("line 5"), "should include console line 5");
        assert!(
            out.contains(r#"{"a":1}"#),
            "should include diagnostics line 1"
        );
        assert!(
            out.contains(r#"{"b":2}"#),
            "should include diagnostics line 2"
        );
    }

    #[test]
    fn console_missing_diagnostics_present() {
        let tmp = make_run_dir();
        let dir = tmp.path();
        write(dir, "diagnostics.jsonl", "{\"event\":\"launch\"}\n");

        let out = captured(dir, 100);

        assert!(
            out.contains("console.log: (not found)"),
            "should note missing console.log"
        );
        assert!(
            out.contains(r#"{"event":"launch"}"#),
            "should include diagnostics"
        );
    }

    #[test]
    fn both_files_missing_no_panic() {
        let tmp = make_run_dir();
        let dir = tmp.path();

        let out = captured(dir, 100);

        assert!(out.contains("console.log: (not found)"));
        assert!(out.contains("diagnostics.jsonl: (not found)"));
    }

    #[test]
    fn console_fewer_lines_than_tail_all_included() {
        let tmp = make_run_dir();
        let dir = tmp.path();
        // 5 lines, tail cap = 100 → all included, no truncation marker.
        let content = (1..=5)
            .map(|i| format!("L{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        write(dir, "console.log", &content);

        let out = captured(dir, 100);

        for i in 1..=5 {
            assert!(out.contains(&format!("L{i}")), "line {i} must appear");
        }
        assert!(
            !out.contains("showing last"),
            "truncation marker must not appear when all lines fit"
        );
    }

    #[test]
    fn console_more_lines_than_tail_shows_only_last_n_with_marker() {
        let tmp = make_run_dir();
        let dir = tmp.path();
        // 200 lines, cap = 10 → lines 191-200 appear; lines 1-190 do not.
        let content = (1..=200u32)
            .map(|i| format!("LINE{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        write(dir, "console.log", &content);

        let out = captured(dir, 10);

        assert!(
            out.contains("showing last 10 of 200 lines"),
            "truncation marker required"
        );
        assert!(out.contains("LINE200"), "last line must appear");
        assert!(out.contains("LINE191"), "191st line must appear (200-10+1)");
        assert!(!out.contains("LINE190"), "190th line must NOT appear");
        assert!(!out.contains("LINE1\n"), "first line must NOT appear");
    }

    // ── RunDirDumpGuard happy-path ────────────────────────────────────────────

    /// Construct + drop a guard without panicking.  The guard must not blow
    /// up, must not produce spurious output, and the test must pass.
    #[test]
    fn guard_drop_without_panic_does_not_blow_up() {
        let tmp = make_run_dir();
        let _guard = RunDirDumpGuard::new(tmp.path().to_path_buf());
        // Drops here — no panic in flight, so Drop does nothing.
    }

    /// `with_tail_lines` stores the requested value.
    #[test]
    fn guard_with_tail_lines_stores_value() {
        let tmp = make_run_dir();
        let guard = RunDirDumpGuard::new(tmp.path().to_path_buf()).with_tail_lines(42);
        assert_eq!(guard.console_tail_lines, 42);
    }

    // ── panic-path via dump_run_dir directly ──────────────────────────────────

    /// The panic-path of Drop calls `dump_run_dir`; we test that function
    /// directly through `catch_unwind` to verify the output without
    /// capturing real stderr.
    #[test]
    fn dump_produces_expected_output_in_panic_path_simulation() {
        let tmp = make_run_dir();
        let dir = tmp.path();
        // 200-line console.log + a few JSONL records.
        let console = (1..=200u32)
            .map(|i| format!("console line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        write(dir, "console.log", &console);
        write(
            dir,
            "diagnostics.jsonl",
            "{\"phase\":\"boot\"}\n{\"phase\":\"ready\"}\n",
        );

        // Simulate what Drop does — capture to a Vec<u8> for assertion.
        let result = std::panic::catch_unwind(|| {
            let mut buf: Vec<u8> = Vec::new();
            dump_run_dir(dir, 50, &mut buf).unwrap();
            String::from_utf8(buf).unwrap()
        });

        let out = result.expect("dump_run_dir must not panic");
        assert!(out.contains("showing last 50 of 200 lines"));
        assert!(out.contains("console line 200"));
        assert!(out.contains("console line 151"));
        assert!(!out.contains("console line 150\n"));
        assert!(out.contains(r#"{"phase":"boot"}"#));
        assert!(out.contains(r#"{"phase":"ready"}"#));
    }
}
