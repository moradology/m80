//! Shared helpers for m80-vsock integration tests.

use std::path::{Path, PathBuf};

/// Write a console-log fixture containing `GUESTD_READY` so
/// `Channel::open`'s marker watch returns immediately.
pub fn console_with_marker(dir: &Path) -> PathBuf {
    let console = dir.join("console.log");
    std::fs::write(&console, "GUESTD_READY\n").unwrap();
    console
}
