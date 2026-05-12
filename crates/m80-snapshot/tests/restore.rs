//! Integration tests for [`m80_snapshot::restore`].
//!
//! Each scenario is its own `#[test]`. The fixture server accepts connections
//! in sequence (one per `FirecrackerClient::new` → one `UnixStream`), serves
//! the prescribed response, and returns raw requests for assertion.
//!
//! Vsock UDS removal is tested directly against the filesystem using a
//! temp dir — no Firecracker involvement needed for that scenario.

mod fixture_server;
use fixture_server::{resp_204, resp_400, FixtureServer};

use std::path::PathBuf;

use m80_snapshot::{restore, RestoreRequest, SnapshotError, SnapshotPaths};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn paths() -> SnapshotPaths {
    SnapshotPaths {
        vm_state: PathBuf::from("/run/m80/vms/vm-1/snapshots/vm.snap"),
        mem: PathBuf::from("/run/m80/vms/vm-1/snapshots/mem.snap"),
    }
}

// ---------------------------------------------------------------------------
// Happy path — no resume
// ---------------------------------------------------------------------------

/// `restore` without resume issues `PUT /snapshot/load` only; no `PATCH /vm`.
#[test]
fn restore_no_resume_sends_load_only() {
    let dir = tempfile::tempdir().unwrap();
    // vsock_uds does not exist — that's fine (NotFound is silently ignored).
    let vsock_uds = dir.path().join("vsock.sock");

    // Only one response needed: PUT /snapshot/load.
    let server = FixtureServer::spawn(vec![resp_204()]).unwrap();

    let req = RestoreRequest {
        api_socket: server.socket_path.clone(),
        paths: paths(),
        vsock_uds,
        resume: false,
    };
    restore(req).expect("restore must succeed");

    let requests = server.join();
    assert_eq!(
        requests.len(),
        1,
        "exactly one HTTP exchange expected (no resume)"
    );
    assert!(
        requests[0].starts_with("PUT /snapshot/load HTTP/1.1\r\n"),
        "sole request must be PUT /snapshot/load, got: {:?}",
        requests[0].lines().next()
    );
}

// ---------------------------------------------------------------------------
// Happy path — with resume
// ---------------------------------------------------------------------------

/// `restore` with `resume: true` issues `PUT /snapshot/load` then
/// `PATCH /vm {"state":"Resumed"}` in that order.
#[test]
fn restore_with_resume_sends_load_then_resumed() {
    let dir = tempfile::tempdir().unwrap();
    let vsock_uds = dir.path().join("vsock.sock");

    let server = FixtureServer::spawn(vec![resp_204(), resp_204()]).unwrap();

    let req = RestoreRequest {
        api_socket: server.socket_path.clone(),
        paths: paths(),
        vsock_uds,
        resume: true,
    };
    restore(req).expect("restore must succeed");

    let requests = server.join();
    assert_eq!(
        requests.len(),
        2,
        "exactly two HTTP exchanges expected (load + resume)"
    );

    assert!(
        requests[0].starts_with("PUT /snapshot/load HTTP/1.1\r\n"),
        "first call must be PUT /snapshot/load, got: {:?}",
        requests[0].lines().next()
    );
    assert!(
        requests[1].starts_with("PATCH /vm HTTP/1.1\r\n"),
        "second call must be PATCH /vm, got: {:?}",
        requests[1].lines().next()
    );
    assert!(
        requests[1].contains("\"state\":\"Resumed\""),
        "PATCH /vm body must carry state=Resumed: {}",
        requests[1]
    );
}

// ---------------------------------------------------------------------------
// Load body shape
// ---------------------------------------------------------------------------

/// `PUT /snapshot/load` body must carry `snapshot_path`, a `mem_backend` with
/// `backend_type: "File"`, and the correct `backend_path`.
#[test]
fn restore_load_body_uses_file_backed_mem_backend() {
    let dir = tempfile::tempdir().unwrap();
    let vsock_uds = dir.path().join("vsock.sock");

    let server = FixtureServer::spawn(vec![resp_204()]).unwrap();

    let req = RestoreRequest {
        api_socket: server.socket_path.clone(),
        paths: SnapshotPaths {
            vm_state: PathBuf::from("/snap/vm.snap"),
            mem: PathBuf::from("/snap/mem.snap"),
        },
        vsock_uds,
        resume: false,
    };
    restore(req).expect("restore must succeed");

    let requests = server.join();
    let body = &requests[0];
    assert!(
        body.contains("\"snapshot_path\""),
        "must include snapshot_path: {body}"
    );
    assert!(
        body.contains("vm.snap"),
        "snapshot_path value must appear: {body}"
    );
    assert!(
        body.contains("\"mem_backend\""),
        "must include mem_backend: {body}"
    );
    assert!(
        body.contains("\"backend_type\":\"File\""),
        "backend_type must be File: {body}"
    );
    assert!(
        body.contains("mem.snap"),
        "backend_path value must appear: {body}"
    );
    assert!(
        !body.contains("\"mem_file_path\""),
        "deprecated mem_file_path must not appear: {body}"
    );
    assert!(
        !body.contains("\"resume_vm\""),
        "resume_vm must not appear in load body (resume is separate PATCH): {body}"
    );
}

// ---------------------------------------------------------------------------
// vsock UDS removal
// ---------------------------------------------------------------------------

/// If the vsock UDS is absent, `restore` proceeds without error.
#[test]
fn restore_absent_vsock_uds_is_not_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let vsock_uds = dir.path().join("does-not-exist.sock");

    let server = FixtureServer::spawn(vec![resp_204()]).unwrap();

    let req = RestoreRequest {
        api_socket: server.socket_path.clone(),
        paths: paths(),
        vsock_uds,
        resume: false,
    };
    restore(req).expect("absent vsock UDS must not be an error");
    server.join();
}

/// If the vsock UDS is present, `restore` removes it before calling load.
#[test]
fn restore_removes_existing_vsock_uds_before_load() {
    let dir = tempfile::tempdir().unwrap();
    let vsock_uds = dir.path().join("vsock.sock");
    // Create a regular file to stand in for the socket file.
    std::fs::write(&vsock_uds, b"stale").unwrap();
    assert!(vsock_uds.exists());

    let server = FixtureServer::spawn(vec![resp_204()]).unwrap();

    let req = RestoreRequest {
        api_socket: server.socket_path.clone(),
        paths: paths(),
        vsock_uds: vsock_uds.clone(),
        resume: false,
    };
    restore(req).expect("restore must succeed");
    server.join();

    assert!(
        !vsock_uds.exists(),
        "vsock UDS must have been removed by restore"
    );
}

/// If the vsock UDS cannot be removed (permission denied), `restore` returns
/// `SnapshotError::VsockUdsUnlink` without calling Firecracker.
#[test]
fn restore_vsock_uds_unlink_failure_surfaces_error() {
    // Create a directory at the vsock_uds path so `remove_file` fails with
    // `IsADirectory` (EISDIR) — not NotFound, so it must surface.
    let dir = tempfile::tempdir().unwrap();
    let vsock_uds = dir.path().join("vsock.sock");
    std::fs::create_dir(&vsock_uds).unwrap();

    // The server does not need to be set up because the error should happen
    // before the client is opened.
    let server = FixtureServer::spawn(vec![]).unwrap();

    let req = RestoreRequest {
        api_socket: server.socket_path.clone(),
        paths: paths(),
        vsock_uds: vsock_uds,
        resume: false,
    };
    let err = restore(req).unwrap_err();

    // Drain the server (no requests were sent).
    let requests = server.join();
    assert_eq!(requests.len(), 0, "no HTTP call should have been made");

    assert!(
        matches!(err, SnapshotError::VsockUdsUnlink { .. }),
        "unlink failure must surface as VsockUdsUnlink, got {err:?}"
    );
}

// ---------------------------------------------------------------------------
// Error paths
// ---------------------------------------------------------------------------

/// If `PUT /snapshot/load` returns 400, `restore` returns
/// `SnapshotError::Client`.
#[test]
fn restore_load_failure_returns_client_error() {
    let dir = tempfile::tempdir().unwrap();
    let vsock_uds = dir.path().join("vsock.sock");

    let fault = r#"{"fault_message":"vsock uds path already in use"}"#;
    let server = FixtureServer::spawn(vec![resp_400(fault)]).unwrap();

    let req = RestoreRequest {
        api_socket: server.socket_path.clone(),
        paths: paths(),
        vsock_uds,
        resume: false,
    };
    let err = restore(req).unwrap_err();

    server.join();
    assert!(
        matches!(err, SnapshotError::Client(_)),
        "load failure must surface as Client error, got {err:?}"
    );
}

/// If `PATCH /vm {"state":"Resumed"}` returns 400, `restore` returns
/// `SnapshotError::Client`.
#[test]
fn restore_resume_failure_returns_client_error() {
    let dir = tempfile::tempdir().unwrap();
    let vsock_uds = dir.path().join("vsock.sock");

    let fault = r#"{"fault_message":"cannot resume from this state"}"#;
    // Load succeeds, resume fails.
    let server = FixtureServer::spawn(vec![resp_204(), resp_400(fault)]).unwrap();

    let req = RestoreRequest {
        api_socket: server.socket_path.clone(),
        paths: paths(),
        vsock_uds,
        resume: true,
    };
    let err = restore(req).unwrap_err();

    server.join();
    assert!(
        matches!(err, SnapshotError::Client(_)),
        "resume failure must surface as Client error, got {err:?}"
    );
}
