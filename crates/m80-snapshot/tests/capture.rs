//! Integration tests for [`m80_snapshot::capture`].
//!
//! Each scenario is its own `#[test]`. The fixture server accepts connections
//! in sequence (one per `FirecrackerClient::new` → one `UnixStream`), serves
//! the prescribed response, and returns raw requests for assertion.

mod fixture_server;
use fixture_server::{resp_204, resp_400, FixtureServer};

use std::path::PathBuf;

use m80_snapshot::{
    capture, CaptureRequest, SnapshotError, SnapshotKind, SnapshotPaths, SNAPSHOT_MANIFEST_FILE,
};

const FC_VERSION: &str = "v1.15.1";

fn prepared_paths() -> (tempfile::TempDir, SnapshotPaths) {
    let dir = tempfile::tempdir().unwrap();
    let paths = SnapshotPaths {
        vm_state: dir.path().join("vm.snap"),
        mem: dir.path().join("mem.snap"),
    };
    std::fs::write(&paths.vm_state, b"vm-state").unwrap();
    std::fs::write(&paths.mem, b"memory").unwrap();
    (dir, paths)
}

fn request(api_socket: PathBuf, paths: SnapshotPaths, kind: SnapshotKind) -> CaptureRequest {
    CaptureRequest {
        api_socket,
        paths: paths.clone(),
        host_paths: paths,
        expected_firecracker_version: FC_VERSION.to_owned(),
        kind,
        enable_diff_snapshots: matches!(kind, SnapshotKind::Diff),
    }
}

// ---------------------------------------------------------------------------
// Happy path — Full snapshot
// ---------------------------------------------------------------------------

/// `capture` must first issue `PATCH /vm`, then `PUT /snapshot/create` — in
/// that order. The VM is left paused after a successful call.
#[test]
fn capture_full_sends_pause_then_create_in_order() {
    // Two responses: one for PATCH /vm, one for PUT /snapshot/create.
    let server = FixtureServer::spawn(vec![resp_204(), resp_204()]).unwrap();
    let (_dir, paths) = prepared_paths();

    let req = request(server.socket_path.clone(), paths, SnapshotKind::Full);
    capture(req).expect("capture must succeed");

    let requests = server.join();
    assert_eq!(requests.len(), 2, "exactly two HTTP exchanges expected");
    assert!(
        requests[0].starts_with("PATCH /vm HTTP/1.1\r\n"),
        "first call must be PATCH /vm, got: {:?}",
        requests[0].lines().next()
    );
    assert!(
        requests[1].starts_with("PUT /snapshot/create HTTP/1.1\r\n"),
        "second call must be PUT /snapshot/create, got: {:?}",
        requests[1].lines().next()
    );
}

/// `PATCH /vm` body carries `state=Paused` to freeze the VM before snapshot.
#[test]
fn capture_pause_request_carries_paused_state() {
    let server = FixtureServer::spawn(vec![resp_204(), resp_204()]).unwrap();
    let (_dir, paths) = prepared_paths();

    let req = request(server.socket_path.clone(), paths, SnapshotKind::Full);
    capture(req).expect("capture must succeed");

    let requests = server.join();
    assert!(
        requests[0].contains("\"state\":\"Paused\""),
        "PATCH /vm body must carry state=Paused: {}",
        requests[0]
    );
}

/// `capture` with `SnapshotKind::Full` includes `snapshot_type: "Full"` in
/// the create body.
#[test]
fn capture_full_kind_serializes_snapshot_type_full() {
    let server = FixtureServer::spawn(vec![resp_204(), resp_204()]).unwrap();
    let (_dir, paths) = prepared_paths();

    let req = request(server.socket_path.clone(), paths, SnapshotKind::Full);
    capture(req).expect("capture must succeed");

    let requests = server.join();
    assert!(
        requests[1].contains("\"snapshot_type\":\"Full\""),
        "create body must contain snapshot_type=Full: {}",
        requests[1]
    );
}

/// `capture` with `SnapshotKind::Diff` includes `snapshot_type: "Diff"` in
/// the create body.
#[test]
fn capture_diff_kind_serializes_snapshot_type_diff() {
    let server = FixtureServer::spawn(vec![resp_204(), resp_204()]).unwrap();
    let (_dir, paths) = prepared_paths();

    let req = request(server.socket_path.clone(), paths, SnapshotKind::Diff);
    capture(req).expect("capture must succeed");

    let requests = server.join();
    assert!(
        requests[1].contains("\"snapshot_type\":\"Diff\""),
        "create body must contain snapshot_type=Diff: {}",
        requests[1]
    );
}

#[test]
fn capture_diff_without_dirty_tracking_fails_before_rest_calls() {
    let server = FixtureServer::spawn(Vec::new()).unwrap();
    let (_dir, paths) = prepared_paths();

    let mut req = request(server.socket_path.clone(), paths, SnapshotKind::Diff);
    req.enable_diff_snapshots = false;
    let err = capture(req).expect_err("diff capture without dirty tracking must fail");

    let requests = server.join();
    assert!(requests.is_empty(), "no REST calls expected: {requests:?}");
    assert!(
        matches!(err, SnapshotError::DiffSnapshotsDisabled),
        "unexpected error: {err:?}"
    );
}

/// `capture` sends the correct `snapshot_path` and `mem_file_path` in the
/// create body.
#[test]
fn capture_sends_correct_paths_in_create_body() {
    let server = FixtureServer::spawn(vec![resp_204(), resp_204()]).unwrap();
    let (_dir, paths) = prepared_paths();

    let req = request(server.socket_path.clone(), paths, SnapshotKind::Full);
    capture(req).expect("capture must succeed");

    let requests = server.join();
    let create_body = &requests[1];
    assert!(
        create_body.contains("vm.snap"),
        "create body must contain vm_state path: {create_body}"
    );
    assert!(
        create_body.contains("mem.snap"),
        "create body must contain mem path: {create_body}"
    );
}

#[test]
fn capture_writes_manifest_after_snapshot_create() {
    let server = FixtureServer::spawn(vec![resp_204(), resp_204()]).unwrap();
    let (dir, paths) = prepared_paths();

    let req = request(server.socket_path.clone(), paths, SnapshotKind::Full);
    capture(req).expect("capture must succeed");

    server.join();
    let manifest_path = dir.path().join(SNAPSHOT_MANIFEST_FILE);
    let manifest = std::fs::read_to_string(&manifest_path).unwrap();
    assert!(manifest.contains("\"expected_firecracker_version\": \"v1.15.1\""));
    assert!(manifest.contains("\"kind\": \"memory\""));
    assert!(manifest.contains("\"kind\": \"vm_state\""));
}

// ---------------------------------------------------------------------------
// Error paths
// ---------------------------------------------------------------------------

/// If `PATCH /vm` returns 400, `capture` returns `SnapshotError::Client`
/// without issuing `PUT /snapshot/create`.
#[test]
fn capture_pause_failure_returns_client_error() {
    let fault = r#"{"fault_message":"vm not in a pausable state"}"#;
    // Only one response — pause fails, create must not be sent.
    let server = FixtureServer::spawn(vec![resp_400(fault)]).unwrap();
    let (_dir, paths) = prepared_paths();

    let req = request(server.socket_path.clone(), paths, SnapshotKind::Full);
    let err = capture(req).unwrap_err();

    server.join();
    assert!(
        matches!(err, SnapshotError::Client(_)),
        "pause failure must surface as Client error, got {err:?}"
    );
}

/// If `PUT /snapshot/create` returns 400, `capture` returns
/// `SnapshotError::Client`.
#[test]
fn capture_create_failure_returns_client_error() {
    let fault = r#"{"fault_message":"vm must be paused before snapshot"}"#;
    let server = FixtureServer::spawn(vec![resp_204(), resp_400(fault)]).unwrap();
    let (_dir, paths) = prepared_paths();

    let req = request(server.socket_path.clone(), paths, SnapshotKind::Full);
    let err = capture(req).unwrap_err();

    server.join();
    assert!(
        matches!(err, SnapshotError::Client(_)),
        "create failure must surface as Client error, got {err:?}"
    );
}
