//! Integration tests for [`m80_snapshot::capture`].
//!
//! Each scenario is its own `#[test]`. The fixture server accepts connections
//! in sequence (one per `FirecrackerClient::new` → one `UnixStream`), serves
//! the prescribed response, and returns raw requests for assertion.

mod fixture_server;
use fixture_server::{resp_204, resp_400, FixtureServer};

use std::path::PathBuf;

use m80_snapshot::{capture, CaptureRequest, SnapshotError, SnapshotKind, SnapshotPaths};

// ---------------------------------------------------------------------------
// Happy path — Full snapshot
// ---------------------------------------------------------------------------

/// `capture` must first issue `PATCH /vm` with `state=Paused`, then
/// `PUT /snapshot/create` with the correct paths. The VM is left paused.
#[test]
fn capture_full_sends_pause_then_create_in_order() {
    // Two responses: one for PATCH /vm, one for PUT /snapshot/create.
    let server = FixtureServer::spawn(vec![resp_204(), resp_204()]).unwrap();

    let req = CaptureRequest {
        fc_socket: server.socket_path.clone(),
        paths: SnapshotPaths {
            vm_state: PathBuf::from("/run/m80/vms/vm-1/snapshots/vm.snap"),
            mem: PathBuf::from("/run/m80/vms/vm-1/snapshots/mem.snap"),
        },
        kind: SnapshotKind::Full,
    };
    capture(req).expect("capture must succeed");

    let requests = server.join();
    assert_eq!(requests.len(), 2, "exactly two HTTP exchanges expected");

    // First request: PATCH /vm Paused
    assert!(
        requests[0].starts_with("PATCH /vm HTTP/1.1\r\n"),
        "first call must be PATCH /vm, got: {:?}",
        requests[0].lines().next()
    );
    assert!(
        requests[0].contains("\"state\":\"Paused\""),
        "PATCH /vm body must carry state=Paused: {}",
        requests[0]
    );

    // Second request: PUT /snapshot/create
    assert!(
        requests[1].starts_with("PUT /snapshot/create HTTP/1.1\r\n"),
        "second call must be PUT /snapshot/create, got: {:?}",
        requests[1].lines().next()
    );
}

/// `capture` with `SnapshotKind::Full` includes `snapshot_type: "Full"` in
/// the create body.
#[test]
fn capture_full_kind_serializes_snapshot_type_full() {
    let server = FixtureServer::spawn(vec![resp_204(), resp_204()]).unwrap();

    let req = CaptureRequest {
        fc_socket: server.socket_path.clone(),
        paths: SnapshotPaths {
            vm_state: PathBuf::from("/snap/vm.snap"),
            mem: PathBuf::from("/snap/mem.snap"),
        },
        kind: SnapshotKind::Full,
    };
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

    let req = CaptureRequest {
        fc_socket: server.socket_path.clone(),
        paths: SnapshotPaths {
            vm_state: PathBuf::from("/snap/vm.snap"),
            mem: PathBuf::from("/snap/mem.snap"),
        },
        kind: SnapshotKind::Diff,
    };
    capture(req).expect("capture must succeed");

    let requests = server.join();
    assert!(
        requests[1].contains("\"snapshot_type\":\"Diff\""),
        "create body must contain snapshot_type=Diff: {}",
        requests[1]
    );
}

/// `capture` sends the correct `snapshot_path` and `mem_file_path` in the
/// create body.
#[test]
fn capture_sends_correct_paths_in_create_body() {
    let server = FixtureServer::spawn(vec![resp_204(), resp_204()]).unwrap();

    let req = CaptureRequest {
        fc_socket: server.socket_path.clone(),
        paths: SnapshotPaths {
            vm_state: PathBuf::from("/run/fc/snap-state.bin"),
            mem: PathBuf::from("/run/fc/snap-mem.bin"),
        },
        kind: SnapshotKind::Full,
    };
    capture(req).expect("capture must succeed");

    let requests = server.join();
    let create_body = &requests[1];
    assert!(
        create_body.contains("snap-state.bin"),
        "create body must contain vm_state path: {create_body}"
    );
    assert!(
        create_body.contains("snap-mem.bin"),
        "create body must contain mem path: {create_body}"
    );
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

    let req = CaptureRequest {
        fc_socket: server.socket_path.clone(),
        paths: SnapshotPaths {
            vm_state: PathBuf::from("/snap/vm.snap"),
            mem: PathBuf::from("/snap/mem.snap"),
        },
        kind: SnapshotKind::Full,
    };
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

    let req = CaptureRequest {
        fc_socket: server.socket_path.clone(),
        paths: SnapshotPaths {
            vm_state: PathBuf::from("/snap/vm.snap"),
            mem: PathBuf::from("/snap/mem.snap"),
        },
        kind: SnapshotKind::Full,
    };
    let err = capture(req).unwrap_err();

    server.join();
    assert!(
        matches!(err, SnapshotError::Client(_)),
        "create failure must surface as Client error, got {err:?}"
    );
}
