//! Tests for `put_snapshot_create`, `put_snapshot_load`, and `patch_vm_state`.
//!
//! Each scenario is its own `#[test]`. The fixture server (defined in
//! `fixture_server.rs`) binds a real `UnixListener`, accepts one connection,
//! serves the prescribed response, and returns the raw request text for
//! assertion.

mod fixture_server;
use fixture_server::{resp_400, setup_with_204, FixtureServer};

use m80_firecracker_client::{
    Client, ClientError, CreateSnapshotConfig, FirecrackerVersion, LoadSnapshotConfig,
    MemBackendConfig, MemBackendType, SnapshotType, VmState, VsockOverride,
};
use std::path::PathBuf;

const SNAP_PATH: &str = "/run/fc/snap.bin";
const MEM_PATH: &str = "/run/fc/mem.bin";
const VSOCK_PATH: &str = "/run/fc/slot-7/vsock.sock";

fn default_load_config() -> LoadSnapshotConfig {
    LoadSnapshotConfig {
        snapshot_path: PathBuf::from(SNAP_PATH),
        mem_backend: Some(MemBackendConfig {
            backend_type: MemBackendType::File,
            backend_path: PathBuf::from(MEM_PATH),
        }),
        mem_file_path: None,
        enable_diff_snapshots: None,
        resume_vm: None,
        vsock_override: None,
    }
}

fn resp_version(version: &str) -> Vec<u8> {
    let body = format!(r#"{{"firecracker_version":"{version}"}}"#);
    format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body
    )
    .into_bytes()
}

// ---------------------------------------------------------------------------
// patch_vm_state — happy paths
// ---------------------------------------------------------------------------

#[test]
fn patch_vm_state_paused_sends_correct_json_and_url() {
    let (server, client) = setup_with_204();
    client.patch_vm_state(VmState::Paused).unwrap();
    let result = server.join();
    assert!(
        result.request.starts_with("PATCH /vm HTTP/1.1\r\n"),
        "unexpected request line: {:?}",
        result.request.lines().next()
    );
    assert!(
        result.request.contains("\"state\":\"Paused\""),
        "body must carry state=Paused: {}",
        result.request
    );
}

#[test]
fn patch_vm_state_resumed_sends_correct_json() {
    let (server, client) = setup_with_204();
    client.patch_vm_state(VmState::Resumed).unwrap();
    let result = server.join();
    assert!(result.request.contains("\"state\":\"Resumed\""));
}

// ---------------------------------------------------------------------------
// patch_vm_state — error path
// ---------------------------------------------------------------------------

#[test]
fn patch_vm_state_400_returns_vm_state_write_failed() {
    let fault = r#"{"fault_message":"vm not in a pausable state"}"#;
    let server = FixtureServer::spawn(resp_400(fault)).unwrap();
    let client = Client::new(&server.socket_path).unwrap();
    let err = client.patch_vm_state(VmState::Paused).unwrap_err();
    server.join();
    assert!(
        matches!(
            &err,
            ClientError::VmStateWriteFailed { state: VmState::Paused, fault }
            if fault.contains("vm not in a pausable state")
        ),
        "unexpected error: {err:?}"
    );
}

// ---------------------------------------------------------------------------
// put_snapshot_create — happy path
// ---------------------------------------------------------------------------

#[test]
fn put_snapshot_create_sends_correct_url_and_required_fields() {
    let (server, client) = setup_with_204();
    client
        .put_snapshot_create(&CreateSnapshotConfig {
            snapshot_path: PathBuf::from(SNAP_PATH),
            mem_file_path: PathBuf::from(MEM_PATH),
            snapshot_type: None,
        })
        .unwrap();
    let result = server.join();
    assert!(
        result
            .request
            .starts_with("PUT /snapshot/create HTTP/1.1\r\n"),
        "unexpected request line: {:?}",
        result.request.lines().next()
    );
    assert!(result.request.contains("\"snapshot_path\""));
    assert!(result.request.contains(SNAP_PATH));
    assert!(result.request.contains("\"mem_file_path\""));
    assert!(result.request.contains(MEM_PATH));
}

#[test]
fn put_snapshot_create_omits_snapshot_type_when_none() {
    let (server, client) = setup_with_204();
    client
        .put_snapshot_create(&CreateSnapshotConfig {
            snapshot_path: PathBuf::from(SNAP_PATH),
            mem_file_path: PathBuf::from(MEM_PATH),
            snapshot_type: None,
        })
        .unwrap();
    let result = server.join();
    assert!(
        !result.request.contains("\"snapshot_type\""),
        "None snapshot_type must be omitted from the body"
    );
}

#[test]
fn put_snapshot_create_includes_snapshot_type_when_set() {
    let (server, client) = setup_with_204();
    client
        .put_snapshot_create(&CreateSnapshotConfig {
            snapshot_path: PathBuf::from(SNAP_PATH),
            mem_file_path: PathBuf::from(MEM_PATH),
            snapshot_type: Some(SnapshotType::Full),
        })
        .unwrap();
    let result = server.join();
    assert!(
        result.request.contains("\"snapshot_type\":\"Full\""),
        "snapshot_type Full must appear in body"
    );
}

#[test]
fn put_snapshot_create_diff_type_serializes_correctly() {
    let (server, client) = setup_with_204();
    client
        .put_snapshot_create(&CreateSnapshotConfig {
            snapshot_path: PathBuf::from(SNAP_PATH),
            mem_file_path: PathBuf::from(MEM_PATH),
            snapshot_type: Some(SnapshotType::Diff),
        })
        .unwrap();
    let result = server.join();
    assert!(result.request.contains("\"snapshot_type\":\"Diff\""));
}

// ---------------------------------------------------------------------------
// put_snapshot_create — error paths
// ---------------------------------------------------------------------------

#[test]
fn put_snapshot_create_400_returns_snapshot_create_failed() {
    let fault = r#"{"fault_message":"vm must be paused before snapshot"}"#;
    let server = FixtureServer::spawn(resp_400(fault)).unwrap();
    let client = Client::new(&server.socket_path).unwrap();
    let err = client
        .put_snapshot_create(&CreateSnapshotConfig {
            snapshot_path: PathBuf::from(SNAP_PATH),
            mem_file_path: PathBuf::from(MEM_PATH),
            snapshot_type: None,
        })
        .unwrap_err();
    server.join();
    assert!(
        matches!(
            &err,
            ClientError::SnapshotCreateFailed { fault }
            if fault.contains("vm must be paused before snapshot")
        ),
        "unexpected error: {err:?}"
    );
}

// ---------------------------------------------------------------------------
// put_snapshot_load — happy paths
// ---------------------------------------------------------------------------

#[test]
fn put_snapshot_load_with_mem_backend_sends_correct_url_and_fields() {
    let (server, client) = setup_with_204();
    client.put_snapshot_load(&default_load_config()).unwrap();
    let result = server.join();
    assert!(
        result
            .request
            .starts_with("PUT /snapshot/load HTTP/1.1\r\n"),
        "unexpected request line: {:?}",
        result.request.lines().next()
    );
    assert!(result.request.contains("\"snapshot_path\""));
    assert!(result.request.contains(SNAP_PATH));
    assert!(result.request.contains("\"mem_backend\""));
    assert!(result.request.contains("\"backend_type\":\"File\""));
    assert!(result.request.contains(MEM_PATH));
}

#[test]
fn put_snapshot_load_omits_optional_fields_when_none() {
    let (server, client) = setup_with_204();
    client.put_snapshot_load(&default_load_config()).unwrap();
    let result = server.join();
    assert!(
        !result.request.contains("\"mem_file_path\""),
        "None mem_file_path must be omitted"
    );
    assert!(
        !result.request.contains("\"enable_diff_snapshots\""),
        "None enable_diff_snapshots must be omitted"
    );
    assert!(
        !result.request.contains("\"resume_vm\""),
        "None resume_vm must be omitted"
    );
    assert!(
        !result.request.contains("\"vsock_override\""),
        "None vsock_override must be omitted"
    );
}

#[test]
fn put_snapshot_load_resume_vm_true_serializes_correctly() {
    let (server, client) = setup_with_204();
    client
        .put_snapshot_load(&LoadSnapshotConfig {
            snapshot_path: PathBuf::from(SNAP_PATH),
            mem_backend: Some(MemBackendConfig {
                backend_type: MemBackendType::File,
                backend_path: PathBuf::from(MEM_PATH),
            }),
            mem_file_path: None,
            enable_diff_snapshots: None,
            resume_vm: Some(true),
            vsock_override: None,
        })
        .unwrap();
    let result = server.join();
    assert!(result.request.contains("\"resume_vm\":true"));
}

#[test]
fn put_snapshot_load_vsock_override_serializes_correctly() {
    let (server, client) = setup_with_204();
    client
        .put_snapshot_load(&LoadSnapshotConfig {
            snapshot_path: PathBuf::from(SNAP_PATH),
            mem_backend: Some(MemBackendConfig {
                backend_type: MemBackendType::File,
                backend_path: PathBuf::from(MEM_PATH),
            }),
            mem_file_path: None,
            enable_diff_snapshots: None,
            resume_vm: None,
            vsock_override: Some(VsockOverride {
                uds_path: PathBuf::from(VSOCK_PATH),
            }),
        })
        .unwrap();
    let result = server.join();
    assert!(result.request.contains("\"vsock_override\""));
    assert!(result.request.contains(VSOCK_PATH));
}

#[test]
fn put_snapshot_load_with_deprecated_mem_file_path() {
    let (server, client) = setup_with_204();
    client
        .put_snapshot_load(&LoadSnapshotConfig {
            snapshot_path: PathBuf::from(SNAP_PATH),
            mem_backend: None,
            mem_file_path: Some(PathBuf::from(MEM_PATH)),
            enable_diff_snapshots: None,
            resume_vm: None,
            vsock_override: None,
        })
        .unwrap();
    let result = server.join();
    assert!(result.request.contains("\"mem_file_path\""));
    assert!(!result.request.contains("\"mem_backend\""));
}

// ---------------------------------------------------------------------------
// put_snapshot_load — error paths
// ---------------------------------------------------------------------------

#[test]
fn put_snapshot_load_400_returns_snapshot_load_failed() {
    let fault = r#"{"fault_message":"vsock uds path already in use"}"#;
    let server = FixtureServer::spawn(resp_400(fault)).unwrap();
    let client = Client::new(&server.socket_path).unwrap();
    let err = client
        .put_snapshot_load(&default_load_config())
        .unwrap_err();
    server.join();
    assert!(
        matches!(
            &err,
            ClientError::SnapshotLoadFailed { fault }
            if fault.contains("vsock uds path already in use")
        ),
        "unexpected error: {err:?}"
    );
}

// ---------------------------------------------------------------------------
// get_version
// ---------------------------------------------------------------------------

#[test]
fn get_version_sends_correct_url_and_decodes_response() {
    let server = FixtureServer::spawn(resp_version("1.10.0")).unwrap();
    let client = Client::new(&server.socket_path).unwrap();

    let version = client.get_version().unwrap();

    let result = server.join();
    assert!(
        result.request.starts_with("GET /version HTTP/1.1\r\n"),
        "unexpected request line: {:?}",
        result.request.lines().next()
    );
    assert_eq!(
        version,
        FirecrackerVersion {
            firecracker_version: "1.10.0".to_owned()
        }
    );
}

#[test]
fn get_version_400_returns_version_read_failed() {
    let fault = r#"{"fault_message":"version unavailable"}"#;
    let server = FixtureServer::spawn(resp_400(fault)).unwrap();
    let client = Client::new(&server.socket_path).unwrap();

    let err = client.get_version().unwrap_err();

    server.join();
    assert!(
        matches!(
            &err,
            ClientError::VersionReadFailed { fault }
            if fault.contains("version unavailable")
        ),
        "unexpected error: {err:?}"
    );
}
