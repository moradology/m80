//! Shared helpers for tests that need a real e2fsprogs / loop-mount setup.

/// Returns true if the current process is root, otherwise prints a SKIP
/// banner and returns false. The `#[ignore]` attribute is the canonical
/// gate; this helper is a second line of defense for `--ignored` runs on
/// hosts where the user forgot to escalate.
pub fn require_root(test_name: &str) -> bool {
    if nix::unistd::Uid::effective().is_root() {
        true
    } else {
        eprintln!("[{test_name}] SKIP: not running as root");
        false
    }
}
