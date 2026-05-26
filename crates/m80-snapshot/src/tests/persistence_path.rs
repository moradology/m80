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
    )
    .unwrap();
    assert_eq!(
        result,
        Path::new("/snapshots/ws-abc/run-def/1700000000000-aabbccdd"),
        "persistence_path must produce the documented template"
    );
}

/// Pure path construction performs no I/O against the store root. Identifier
/// validation still runs before any path joins.
#[test]
fn no_io_performed_on_nonexistent_root() {
    let result =
        persistence_path(Path::new("/does/not/exist"), "ws", "run", 42, "deadbeef").unwrap();
    // If any I/O were performed this would panic or error on a missing dir.
    assert_eq!(result, Path::new("/does/not/exist/ws/run/42-deadbeef"),);
}

/// Workspace IDs with path-separator characters are rejected before any
/// Path::join call can interpret them as additional components.
#[test]
fn workspace_id_path_separator_is_rejected() {
    let err = persistence_path(Path::new("/s"), "a/b", "r", 1, "ff").unwrap_err();
    assert!(matches!(
        err,
        crate::SnapshotError::InvalidId {
            field: "workspace_id",
            ..
        }
    ));
}
