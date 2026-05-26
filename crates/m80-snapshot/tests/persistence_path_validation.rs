use std::path::Path;

use m80_snapshot::{persistence_path, SnapshotError};

#[test]
fn persistence_path_rejects_path_traversal_ids() {
    let bad_ids = [
        "",
        "../etc",
        "../../etc",
        "/etc/passwd",
        "foo/bar",
        r"foo\bar",
        ".hidden",
        "run..id",
        "nul\0byte",
    ];

    for bad in bad_ids {
        let err = persistence_path(Path::new("/snapshots"), bad, "run", 1, "deadbeef")
            .expect_err("workspace_id must be rejected");
        assert!(
            matches!(err, SnapshotError::InvalidId { field: "workspace_id", ref value } if value == bad),
            "unexpected workspace_id error for {bad:?}: {err:?}"
        );

        let err = persistence_path(Path::new("/snapshots"), "workspace", bad, 1, "deadbeef")
            .expect_err("run_id must be rejected");
        assert!(
            matches!(err, SnapshotError::InvalidId { field: "run_id", ref value } if value == bad),
            "unexpected run_id error for {bad:?}: {err:?}"
        );
    }
}

#[test]
fn persistence_path_accepts_single_component_ids() {
    let path = persistence_path(
        Path::new("/snapshots"),
        "workspace-01",
        "run_02",
        42,
        "deadbeef",
    )
    .unwrap();

    assert_eq!(
        path,
        Path::new("/snapshots/workspace-01/run_02/42-deadbeef")
    );
}
