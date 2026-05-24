//! Integration tests for [`m80_snapshot::restore`].
//!
//! Each scenario is its own `#[test]`. The fixture server accepts connections
//! in sequence (one per `FirecrackerClient::new` → one `UnixStream`), serves
//! the prescribed response, and returns raw requests for assertion.
//!
//! Vsock UDS removal is tested directly against the filesystem using a
//! temp dir — no Firecracker involvement needed for that scenario.

mod fixture_server;
use fixture_server::{resp_204, resp_400, resp_version, FixtureServer};

use std::path::PathBuf;

use m80_snapshot::{
    restore, restore_preverified, write_snapshot_manifest, RestoreRequest, SnapshotError,
    SnapshotPaths,
};

const FC_VERSION: &str = "v1.15.1";
const FC_API_VERSION: &str = "1.15.1";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

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
    resume: bool,
) -> RestoreRequest {
    RestoreRequest {
        api_socket,
        paths: paths.clone(),
        host_paths: paths,
        expected_firecracker_version: FC_VERSION.to_owned(),
        vsock_uds,
        enable_diff_snapshots: false,
        resume,
    }
}

fn with_live_version(mut responses: Vec<Vec<u8>>) -> Vec<Vec<u8>> {
    let mut all = vec![resp_version(FC_API_VERSION)];
    all.append(&mut responses);
    all
}

// ---------------------------------------------------------------------------
// Happy path — no resume
// ---------------------------------------------------------------------------

/// `restore` without resume checks version and issues `PUT /snapshot/load`; no `PATCH /vm`.
#[test]
fn restore_no_resume_checks_version_then_loads() {
    let dir = tempfile::tempdir().unwrap();
    let (_snap_dir, paths) = prepared_paths();
    // vsock_uds does not exist — that's fine (NotFound is silently ignored).
    let vsock_uds = dir.path().join("vsock.sock");

    let server = FixtureServer::spawn(with_live_version(vec![resp_204()])).unwrap();

    let req = request(server.socket_path.clone(), paths, vsock_uds, false);
    restore(req).expect("restore must succeed");

    let requests = server.join();
    assert_eq!(
        requests.len(),
        2,
        "exactly two HTTP exchanges expected (version + no-resume load)"
    );
    assert!(
        requests[0].starts_with("GET /version HTTP/1.1\r\n"),
        "first call must be GET /version, got: {:?}",
        requests[0].lines().next()
    );
    assert!(
        requests[1].starts_with("PUT /snapshot/load HTTP/1.1\r\n"),
        "second call must be PUT /snapshot/load, got: {:?}",
        requests[1].lines().next()
    );
}

// ---------------------------------------------------------------------------
// Happy path — with resume
// ---------------------------------------------------------------------------

/// `restore` with `resume: true` issues `PUT /snapshot/load` then
/// `PATCH /vm {"state":"Resumed"}` in that order.
#[test]
fn restore_with_resume_checks_version_then_loads_and_resumes() {
    let dir = tempfile::tempdir().unwrap();
    let (_snap_dir, paths) = prepared_paths();
    let vsock_uds = dir.path().join("vsock.sock");

    let server = FixtureServer::spawn(with_live_version(vec![resp_204(), resp_204()])).unwrap();

    let req = request(server.socket_path.clone(), paths, vsock_uds, true);
    restore(req).expect("restore must succeed");

    let requests = server.join();
    assert_eq!(
        requests.len(),
        3,
        "exactly three HTTP exchanges expected (version + load + resume)"
    );

    assert!(
        requests[0].starts_with("GET /version HTTP/1.1\r\n"),
        "first call must be GET /version, got: {:?}",
        requests[0].lines().next()
    );
    assert!(
        requests[1].starts_with("PUT /snapshot/load HTTP/1.1\r\n"),
        "second call must be PUT /snapshot/load, got: {:?}",
        requests[1].lines().next()
    );
    assert!(
        requests[2].starts_with("PATCH /vm HTTP/1.1\r\n"),
        "third call must be PATCH /vm, got: {:?}",
        requests[2].lines().next()
    );
    assert!(
        requests[2].contains("\"state\":\"Resumed\""),
        "PATCH /vm body must carry state=Resumed: {}",
        requests[2]
    );
}

// ---------------------------------------------------------------------------
// Load body shape
// ---------------------------------------------------------------------------

/// `PUT /snapshot/load` body carries the `snapshot_path` key with the correct value.
#[test]
fn restore_load_body_includes_snapshot_path() {
    let dir = tempfile::tempdir().unwrap();
    let vsock_uds = dir.path().join("vsock.sock");
    let (_snap_dir, paths) = prepared_paths();
    let server = FixtureServer::spawn(with_live_version(vec![resp_204()])).unwrap();
    let req = request(server.socket_path.clone(), paths, vsock_uds, false);
    restore(req).expect("restore must succeed");
    let requests = server.join();
    let body = &requests[1];
    assert!(
        body.contains("\"snapshot_path\""),
        "must include snapshot_path key: {body}"
    );
    assert!(
        body.contains("vm.snap"),
        "snapshot_path value must appear: {body}"
    );
}

/// `PUT /snapshot/load` body carries a `mem_backend` object with `backend_type: "File"`.
#[test]
fn restore_load_body_uses_file_backed_mem_backend() {
    let dir = tempfile::tempdir().unwrap();
    let vsock_uds = dir.path().join("vsock.sock");
    let (_snap_dir, paths) = prepared_paths();
    let server = FixtureServer::spawn(with_live_version(vec![resp_204()])).unwrap();
    let req = request(server.socket_path.clone(), paths, vsock_uds, false);
    restore(req).expect("restore must succeed");
    let requests = server.join();
    let body = &requests[1];
    assert!(
        body.contains("\"mem_backend\""),
        "must include mem_backend key: {body}"
    );
    assert!(
        body.contains("\"backend_type\":\"File\""),
        "backend_type must be File: {body}"
    );
    assert!(
        body.contains("mem.snap"),
        "backend_path value must appear: {body}"
    );
}

/// `PUT /snapshot/load` body must not include the deprecated `mem_file_path` field.
#[test]
fn restore_load_body_omits_deprecated_mem_file_path() {
    let dir = tempfile::tempdir().unwrap();
    let vsock_uds = dir.path().join("vsock.sock");
    let (_snap_dir, paths) = prepared_paths();
    let server = FixtureServer::spawn(with_live_version(vec![resp_204()])).unwrap();
    let req = request(server.socket_path.clone(), paths, vsock_uds, false);
    restore(req).expect("restore must succeed");
    let requests = server.join();
    let body = &requests[1];
    assert!(
        !body.contains("\"mem_file_path\""),
        "deprecated mem_file_path must not appear: {body}"
    );
}

/// `PUT /snapshot/load` body must not include `resume_vm`; resume is a separate PATCH.
#[test]
fn restore_load_body_omits_resume_vm() {
    let dir = tempfile::tempdir().unwrap();
    let vsock_uds = dir.path().join("vsock.sock");
    let (_snap_dir, paths) = prepared_paths();
    let server = FixtureServer::spawn(with_live_version(vec![resp_204()])).unwrap();
    let req = request(server.socket_path.clone(), paths, vsock_uds, false);
    restore(req).expect("restore must succeed");
    let requests = server.join();
    let body = &requests[1];
    assert!(
        !body.contains("\"resume_vm\""),
        "resume_vm must not appear in load body: {body}"
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
    let (_snap_dir, paths) = prepared_paths();

    let server = FixtureServer::spawn(with_live_version(vec![resp_204()])).unwrap();

    let req = request(server.socket_path.clone(), paths, vsock_uds, false);
    restore(req).expect("absent vsock UDS must not be an error");
    server.join();
}

/// If the vsock UDS is present, `restore` removes it before calling load.
#[test]
fn restore_removes_existing_vsock_uds_before_load() {
    let dir = tempfile::tempdir().unwrap();
    let vsock_uds = dir.path().join("vsock.sock");
    let (_snap_dir, paths) = prepared_paths();
    // Create a regular file to stand in for the socket file.
    std::fs::write(&vsock_uds, b"stale").unwrap();
    assert!(vsock_uds.exists());

    let server = FixtureServer::spawn(with_live_version(vec![resp_204()])).unwrap();

    let req = request(server.socket_path.clone(), paths, vsock_uds.clone(), false);
    restore(req).expect("restore must succeed");
    server.join();

    assert!(
        !vsock_uds.exists(),
        "vsock UDS must have been removed by restore"
    );
}

#[test]
fn restore_rejects_tampered_memory_before_load() {
    let dir = tempfile::tempdir().unwrap();
    let vsock_uds = dir.path().join("vsock.sock");
    let (_snap_dir, paths) = prepared_paths();
    std::fs::write(&paths.mem, b"tampered").unwrap();
    let server = FixtureServer::spawn(vec![]).unwrap();

    let req = request(server.socket_path.clone(), paths, vsock_uds, false);
    let err = restore(req).unwrap_err();

    let requests = server.join();
    assert_eq!(
        requests.len(),
        0,
        "tamper rejection must happen before HTTP"
    );
    assert!(
        matches!(
            err,
            SnapshotError::ArtifactMismatch { .. } | SnapshotError::ArtifactSetMismatch { .. }
        ),
        "tampered memory must surface as an integrity error, got {err:?}"
    );
}

#[test]
fn restore_preverified_skips_artifact_hash_before_load() {
    let dir = tempfile::tempdir().unwrap();
    let vsock_uds = dir.path().join("vsock.sock");
    let (_snap_dir, paths) = prepared_paths();
    std::fs::write(&paths.mem, b"tampered-after-template-pin").unwrap();
    let server = FixtureServer::spawn(with_live_version(vec![resp_204()])).unwrap();

    let req = request(server.socket_path.clone(), paths, vsock_uds, false);
    restore_preverified(req).expect("preverified restore must skip per-restore hash");

    let requests = server.join();
    assert_eq!(
        requests.len(),
        2,
        "preverified restore should check version, then proceed to snapshot load"
    );
    assert!(
        requests[1].starts_with("PUT /snapshot/load HTTP/1.1\r\n"),
        "preverified restore must still issue PUT /snapshot/load"
    );
}

#[test]
fn restore_rejects_firecracker_version_mismatch_before_load() {
    let dir = tempfile::tempdir().unwrap();
    let vsock_uds = dir.path().join("vsock.sock");
    let (_snap_dir, paths) = prepared_paths();
    let server = FixtureServer::spawn(vec![]).unwrap();

    let mut req = request(server.socket_path.clone(), paths, vsock_uds, false);
    req.expected_firecracker_version = "v9.99.0".to_owned();
    let err = restore(req).unwrap_err();

    let requests = server.join();
    assert_eq!(
        requests.len(),
        0,
        "version mismatch must happen before HTTP"
    );
    assert!(
        matches!(err, SnapshotError::FirecrackerVersionMismatch { .. }),
        "version mismatch must surface as typed mismatch, got {err:?}"
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
    let (_snap_dir, paths) = prepared_paths();

    let server = FixtureServer::spawn(vec![resp_version(FC_API_VERSION)]).unwrap();

    let req = request(server.socket_path.clone(), paths, vsock_uds, false);
    let err = restore(req).unwrap_err();

    let requests = server.join();
    assert_eq!(requests.len(), 1, "only the version check should run");
    assert!(
        requests[0].starts_with("GET /version HTTP/1.1\r\n"),
        "version check must precede unlink failure: {:?}",
        requests[0].lines().next()
    );

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
    let (_snap_dir, paths) = prepared_paths();

    let fault = r#"{"fault_message":"vsock uds path already in use"}"#;
    let server = FixtureServer::spawn(with_live_version(vec![resp_400(fault)])).unwrap();

    let req = request(server.socket_path.clone(), paths, vsock_uds, false);
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
    let (_snap_dir, paths) = prepared_paths();

    let fault = r#"{"fault_message":"cannot resume from this state"}"#;
    // Load succeeds, resume fails.
    let server =
        FixtureServer::spawn(with_live_version(vec![resp_204(), resp_400(fault)])).unwrap();

    let req = request(server.socket_path.clone(), paths, vsock_uds, true);
    let err = restore(req).unwrap_err();

    server.join();
    assert!(
        matches!(err, SnapshotError::Client(_)),
        "resume failure must surface as Client error, got {err:?}"
    );
}
