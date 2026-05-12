//! Tests for `persistence_path`.
//! Bead: m80-0tf.2.1

use std::path::Path;

use crate::persistence_path;

/// The function produces the exact documented template:
/// `<store_root>/<workspace_id>/<run_id>/<created_at_unix_ms>-<artifact_set_sha256>/`.
#[test]
fn exact_output_matches_documented_template() {
    let store_root = Path::new("/snapshots");
    let result = persistence_path(
        store_root,
        "ws-abc",
        "run-def",
        1_700_000_000_000,
        "aabbccdd",
    );
    assert_eq!(
        result,
        Path::new("/snapshots/ws-abc/run-def/1700000000000-aabbccdd"),
        "persistence_path must produce the documented template"
    );
}

/// Pure path construction — no I/O, no validation. Exotic IDs pass through
/// unchanged (the caller is responsible for sane inputs).
#[test]
fn no_io_performed_on_nonexistent_root() {
    let result = persistence_path(Path::new("/does/not/exist"), "ws", "run", 42, "deadbeef");
    // If any I/O were performed this would panic or error on a missing dir.
    assert_eq!(result, Path::new("/does/not/exist/ws/run/42-deadbeef"),);
}

/// Workspace IDs with path-separator characters are NOT sanitised — that is
/// the caller's responsibility. This test documents the current (pass-through)
/// behavior to catch accidental changes.
#[test]
fn workspace_id_is_not_sanitised() {
    let result = persistence_path(Path::new("/s"), "a/b", "r", 1, "ff");
    // "a/b" becomes an additional path component.
    assert_eq!(result, Path::new("/s/a/b/r/1-ff"));
}
