//! Dirty-page tracking and diff-snapshot restore request plumbing.

mod fixture_server;
use fixture_server::{resp_204, resp_version, FixtureServer};

use std::path::PathBuf;

use m80_snapshot::{restore, write_snapshot_manifest, RestoreRequest, SnapshotPaths};

const FC_VERSION: &str = "v1.15.1";
const FC_API_VERSION: &str = "1.15.1";

fn prepared_paths() -> (tempfile::TempDir, SnapshotPaths) {
    let dir = tempfile::tempdir().unwrap();
    let paths = SnapshotPaths {
        vm_state: dir.path().join("vm.snap"),
        mem: dir.path().join("mem.snap"),
    };
    std::fs::write(&paths.vm_state, b"vm-state").unwrap();
    std::fs::write(&paths.mem, b"memory").unwrap();
    write_snapshot_manifest(&paths, FC_VERSION).unwrap();
    (dir, paths)
}

fn request(
    api_socket: PathBuf,
    paths: SnapshotPaths,
    vsock_uds: PathBuf,
    enable_diff_snapshots: bool,
) -> RestoreRequest {
    RestoreRequest {
        api_socket,
        paths: paths.clone(),
        host_paths: paths,
        expected_firecracker_version: FC_VERSION.to_owned(),
        vsock_uds,
        enable_diff_snapshots,
        resume: false,
    }
}

#[test]
fn restore_with_enable_diff_snapshots_false_omits_load_field() {
    let dir = tempfile::tempdir().unwrap();
    let (_snap_dir, paths) = prepared_paths();
    let server = FixtureServer::spawn(vec![resp_version(FC_API_VERSION), resp_204()]).unwrap();

    let req = request(
        server.socket_path.clone(),
        paths,
        dir.path().join("vsock.sock"),
        false,
    );
    restore(req).expect("restore must succeed");

    let requests = server.join();
    assert_eq!(requests.len(), 2, "restore must check version then load");
    assert!(
        !requests[1].contains("\"enable_diff_snapshots\""),
        "false must be omitted from snapshot load body: {}",
        requests[1]
    );
}

#[test]
fn restore_with_enable_diff_snapshots_true_sets_load_field() {
    let dir = tempfile::tempdir().unwrap();
    let (_snap_dir, paths) = prepared_paths();
    let server = FixtureServer::spawn(vec![resp_version(FC_API_VERSION), resp_204()]).unwrap();

    let req = request(
        server.socket_path.clone(),
        paths,
        dir.path().join("vsock.sock"),
        true,
    );
    restore(req).expect("restore must succeed");

    let requests = server.join();
    assert_eq!(requests.len(), 2, "restore must check version then load");
    assert!(
        requests[1].contains("\"enable_diff_snapshots\":true"),
        "true must be serialized in snapshot load body: {}",
        requests[1]
    );
}
