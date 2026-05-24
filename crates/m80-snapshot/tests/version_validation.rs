//! Live Firecracker version validation for snapshot restore.

mod fixture_server;
use fixture_server::{resp_204, resp_version, FixtureServer};

use std::path::PathBuf;

use m80_snapshot::{
    restore, write_snapshot_manifest, RestoreRequest, SnapshotError, SnapshotPaths,
};

const FC_VERSION: &str = "v1.10.0";
const FC_API_VERSION: &str = "1.10.0";

fn prepared_paths(expected_firecracker_version: &str) -> (tempfile::TempDir, SnapshotPaths) {
    let dir = tempfile::tempdir().unwrap();
    let paths = SnapshotPaths {
        vm_state: dir.path().join("vm.snap"),
        mem: dir.path().join("mem.snap"),
    };
    std::fs::write(&paths.vm_state, b"vm-state").unwrap();
    std::fs::write(&paths.mem, b"memory").unwrap();
    write_snapshot_manifest(&paths, expected_firecracker_version).unwrap();
    (dir, paths)
}

fn request(
    api_socket: PathBuf,
    paths: SnapshotPaths,
    vsock_uds: PathBuf,
    expected_firecracker_version: &str,
) -> RestoreRequest {
    RestoreRequest {
        api_socket,
        paths: paths.clone(),
        host_paths: paths,
        expected_firecracker_version: expected_firecracker_version.to_owned(),
        vsock_uds,
        enable_diff_snapshots: false,
        resume: false,
    }
}

#[test]
fn version_mismatch_is_rejected_before_load() {
    let dir = tempfile::tempdir().unwrap();
    let vsock_uds = dir.path().join("vsock.sock");
    let expected = "v999.0.0";
    let (_snap_dir, paths) = prepared_paths(expected);
    let server = FixtureServer::spawn(vec![resp_version(FC_API_VERSION)]).unwrap();

    let req = request(server.socket_path.clone(), paths, vsock_uds, expected);
    let err = restore(req).unwrap_err();

    let requests = server.join();
    assert_eq!(requests.len(), 1, "restore must stop after GET /version");
    assert!(
        requests[0].starts_with("GET /version HTTP/1.1\r\n"),
        "first request must be version probe: {:?}",
        requests[0].lines().next()
    );
    assert!(
        matches!(
            err,
            SnapshotError::VersionMismatch {
                ref expected,
                ref actual
            } if expected == "v999.0.0" && actual == FC_VERSION
        ),
        "live version mismatch must be typed, got {err:?}"
    );
}

#[test]
fn version_match_proceeds_to_load() {
    let dir = tempfile::tempdir().unwrap();
    let vsock_uds = dir.path().join("vsock.sock");
    let (_snap_dir, paths) = prepared_paths(FC_VERSION);
    let server = FixtureServer::spawn(vec![resp_version(FC_API_VERSION), resp_204()]).unwrap();

    let req = request(server.socket_path.clone(), paths, vsock_uds, FC_VERSION);
    restore(req).expect("matching live version must proceed to snapshot load");

    let requests = server.join();
    assert_eq!(requests.len(), 2, "restore must check version then load");
    assert!(
        requests[0].starts_with("GET /version HTTP/1.1\r\n"),
        "first request must be version probe: {:?}",
        requests[0].lines().next()
    );
    assert!(
        requests[1].starts_with("PUT /snapshot/load HTTP/1.1\r\n"),
        "second request must load the snapshot: {:?}",
        requests[1].lines().next()
    );
}
