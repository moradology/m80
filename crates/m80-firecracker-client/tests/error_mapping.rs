//! Fixture server returns 400 with a Firecracker fault body for each resource;
//! asserts the corresponding typed `ClientError::*WriteFailed` variant.

mod fixture_server;
use fixture_server::{FixtureServer, resp_400};

use m80_firecracker_client::{
    BootSourceConfig, Client, ClientError, DriveConfig, InstanceAction, MachineConfig,
    NetworkInterfaceConfig, VsockConfig,
};
use std::path::PathBuf;

fn fault_body(msg: &str) -> String {
    format!("{{\"fault_message\":\"{msg}\"}}")
}

#[test]
fn boot_source_400_returns_boot_source_write_failed() {
    let body = fault_body("bad kernel path");
    let server = FixtureServer::spawn(resp_400(&body)).unwrap();
    let client = Client::new(&server.socket_path).unwrap();
    let err = client
        .put_boot_source(&BootSourceConfig {
            kernel_image_path: PathBuf::from("/bad"),
            boot_args: None,
            initrd_path: None,
        })
        .unwrap_err();
    server.join();
    assert!(
        matches!(err, ClientError::BootSourceWriteFailed { ref fault } if fault.contains("bad kernel path")),
        "expected BootSourceWriteFailed, got {err:?}"
    );
}

#[test]
fn machine_config_400_returns_machine_config_write_failed() {
    let body = fault_body("invalid vcpu_count");
    let server = FixtureServer::spawn(resp_400(&body)).unwrap();
    let client = Client::new(&server.socket_path).unwrap();
    let err = client
        .put_machine_config(&MachineConfig { vcpu_count: 0, mem_size_mib: 0, smt: false })
        .unwrap_err();
    server.join();
    assert!(
        matches!(err, ClientError::MachineConfigWriteFailed { ref fault } if fault.contains("invalid vcpu_count")),
        "expected MachineConfigWriteFailed, got {err:?}"
    );
}

#[test]
fn drive_400_returns_drive_write_failed() {
    let body = fault_body("drive not found");
    let server = FixtureServer::spawn(resp_400(&body)).unwrap();
    let client = Client::new(&server.socket_path).unwrap();
    let err = client
        .put_drive(&DriveConfig {
            drive_id: "rootfs".to_owned(),
            path_on_host: PathBuf::from("/missing.ext4"),
            is_root_device: true,
            is_read_only: false,
        })
        .unwrap_err();
    server.join();
    assert!(
        matches!(err, ClientError::DriveWriteFailed { ref fault } if fault.contains("drive not found")),
        "expected DriveWriteFailed, got {err:?}"
    );
}

#[test]
fn network_interface_400_returns_network_interface_write_failed() {
    let body = fault_body("tap device missing");
    let server = FixtureServer::spawn(resp_400(&body)).unwrap();
    let client = Client::new(&server.socket_path).unwrap();
    let err = client
        .put_network_interface(&NetworkInterfaceConfig {
            iface_id: "eth0".to_owned(),
            host_dev_name: "tap0".to_owned(),
            guest_mac: None,
        })
        .unwrap_err();
    server.join();
    assert!(
        matches!(err, ClientError::NetworkInterfaceWriteFailed { ref fault } if fault.contains("tap device missing")),
        "expected NetworkInterfaceWriteFailed, got {err:?}"
    );
}

#[test]
fn vsock_400_returns_vsock_write_failed() {
    let body = fault_body("vsock path in use");
    let server = FixtureServer::spawn(resp_400(&body)).unwrap();
    let client = Client::new(&server.socket_path).unwrap();
    let err = client
        .put_vsock(&VsockConfig { guest_cid: 3, uds_path: PathBuf::from("/run/fc/v.sock") })
        .unwrap_err();
    server.join();
    assert!(
        matches!(err, ClientError::VsockWriteFailed { ref fault } if fault.contains("vsock path in use")),
        "expected VsockWriteFailed, got {err:?}"
    );
}

#[test]
fn instance_action_400_returns_instance_action_failed_with_action() {
    let body = fault_body("cannot start: already running");
    let server = FixtureServer::spawn(resp_400(&body)).unwrap();
    let client = Client::new(&server.socket_path).unwrap();
    let err = client.instance_action(InstanceAction::InstanceStart).unwrap_err();
    server.join();
    assert!(
        matches!(
            err,
            ClientError::InstanceActionFailed {
                action: InstanceAction::InstanceStart,
                ref fault
            } if fault.contains("already running")
        ),
        "expected InstanceActionFailed(InstanceStart), got {err:?}"
    );
}

#[test]
fn connect_error_on_missing_socket() {
    let dir = tempfile::tempdir().unwrap();
    let err = Client::new(&dir.path().join("missing.sock")).unwrap_err();
    assert!(matches!(err, ClientError::Connect(_)), "expected Connect, got {err:?}");
}
