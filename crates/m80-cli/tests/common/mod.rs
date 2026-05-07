//! Shared test helpers for m80-cli integration tests.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

use assert_cmd::Command;

/// Return a [`Command`] pointing at the `m80` binary built by Cargo.
pub fn m80() -> Command {
    Command::cargo_bin("m80").unwrap()
}

/// Collect and sort all entries under `run_root`.
pub fn run_root_entries(run_root: &Path) -> Vec<PathBuf> {
    let mut entries = std::fs::read_dir(run_root)
        .unwrap_or_else(|e| panic!("read run root {}: {e}", run_root.display()))
        .map(|entry| entry.expect("run-root entry").path())
        .collect::<Vec<_>>();
    entries.sort();
    entries
}

/// Append the last `max_lines` lines of `contents` to `out`.
pub fn append_tail(out: &mut String, contents: &str, max_lines: usize) {
    let lines = contents.lines().collect::<Vec<_>>();
    let start = lines.len().saturating_sub(max_lines);
    for line in &lines[start..] {
        out.push_str(line);
        out.push('\n');
    }
}

/// Append the contents of `dir/name` to `out`; silently skips if missing.
pub fn append_file(out: &mut String, dir: &Path, name: &str) {
    let path = dir.join(name);
    match std::fs::read_to_string(&path) {
        Ok(contents) => {
            out.push_str(&format!("--- {name} ---\n"));
            if name == "console.log" {
                append_tail(out, &contents, 100);
            } else {
                out.push_str(&contents);
                if !contents.ends_with('\n') {
                    out.push('\n');
                }
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => out.push_str(&format!("--- {name}: {e} ---\n")),
    }
}
