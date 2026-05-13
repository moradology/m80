//! Fixture server returns 400 with a Firecracker fault body for each resource;
//! asserts the corresponding typed `ClientError::*WriteFailed` variant.

mod fixture_server;
use fixture_server::{resp_400, FixtureServer};

use m80_firecracker_client::{
    BootSourceConfig, Client, ClientError, CpuTemplate, DriveConfig, InstanceAction, MachineConfig,
    PartialDriveConfig, VsockConfig,
};
use std::collections::BTreeMap;
use std::path::PathBuf;

fn fault_body(msg: &str) -> String {
    format!("{{\"fault_message\":\"{msg}\"}}")
}

/// Spawn a fixture-server, build a Client against it, run `call`, and assert
/// the returned error matches `predicate`. Joins the server thread before
/// asserting so any handler panic surfaces.
fn assert_error<F, P>(fault_msg: &str, call: F, predicate: P)
where
    F: FnOnce(&Client) -> Result<(), ClientError>,
    P: FnOnce(&ClientError) -> bool,
{
    let body = fault_body(fault_msg);
    let server = FixtureServer::spawn(resp_400(&body)).unwrap();
    let client = Client::new(&server.socket_path).unwrap();
    let err = call(&client).unwrap_err();
    server.join();
    assert!(predicate(&err), "predicate failed for: {err:?}");
}

#[test]
fn boot_source_400_returns_boot_source_write_failed() {
    assert_error(
        "bad kernel path",
        |c| {
            c.put_boot_source(&BootSourceConfig {
                kernel_image_path: PathBuf::from("/bad"),
                boot_args: None,
                initrd_path: None,
            })
        },
        |e| matches!(e, ClientError::BootSourceWriteFailed { fault } if fault.contains("bad kernel path")),
    );
}

#[test]
fn machine_config_400_returns_machine_config_write_failed() {
    assert_error(
        "invalid vcpu_count",
        |c| {
            c.put_machine_config(&MachineConfig {
                vcpu_count: 0,
                mem_size_mib: 0,
                smt: false,
                cpu_template: Some(CpuTemplate::T2),
            })
        },
        |e| matches!(e, ClientError::MachineConfigWriteFailed { fault } if fault.contains("invalid vcpu_count")),
    );
}

#[test]
fn drive_400_returns_drive_write_failed() {
    assert_error(
        "drive not found",
        |c| {
            c.put_drive(&DriveConfig {
                drive_id: "rootfs".to_owned(),
                path_on_host: PathBuf::from("/missing.ext4"),
                is_root_device: true,
                is_read_only: false,
            })
        },
        |e| matches!(e, ClientError::DriveWriteFailed { fault } if fault.contains("drive not found")),
    );
}

#[test]
fn patch_drive_400_returns_drive_write_failed() {
    assert_error(
        "drive slot update failed",
        |c| {
            c.patch_drive(&PartialDriveConfig {
                drive_id: "workspace_slot_0".to_owned(),
                path_on_host: Some(PathBuf::from("/missing.ext4")),
            })
        },
        |e| matches!(e, ClientError::DriveWriteFailed { fault } if fault.contains("drive slot update failed")),
    );
}

#[test]
fn vsock_400_returns_vsock_write_failed() {
    assert_error(
        "vsock path in use",
        |c| {
            c.put_vsock(&VsockConfig {
                guest_cid: 3,
                uds_path: PathBuf::from("/run/fc/v.sock"),
            })
        },
        |e| matches!(e, ClientError::VsockWriteFailed { fault } if fault.contains("vsock path in use")),
    );
}

#[test]
fn entropy_device_400_returns_entropy_device_write_failed() {
    assert_error(
        "entropy device already configured",
        |c| c.put_entropy_device(),
        |e| matches!(e, ClientError::EntropyDeviceWriteFailed { fault } if fault.contains("already configured")),
    );
}

#[test]
fn instance_action_400_returns_instance_action_failed_with_action() {
    assert_error(
        "cannot start: already running",
        |c| c.instance_action(InstanceAction::InstanceStart),
        |e| {
            matches!(
                e,
                ClientError::InstanceActionFailed {
                    action: InstanceAction::InstanceStart,
                    fault,
                } if fault.contains("already running")
            )
        },
    );
}

#[test]
fn connect_error_on_missing_socket() {
    let dir = tempfile::tempdir().unwrap();
    let err = Client::new(&dir.path().join("missing.sock")).unwrap_err();
    assert!(
        matches!(err, ClientError::Connect(_)),
        "expected Connect, got {err:?}"
    );
}

#[test]
fn serde_json_error_maps_to_serialize_not_io() {
    let mut invalid_json_map_key = BTreeMap::new();
    invalid_json_map_key.insert(vec![1_u8, 2, 3], "value");
    let serde_error = serde_json::to_vec(&invalid_json_map_key).unwrap_err();

    let err = ClientError::from(serde_error);

    assert!(
        matches!(err, ClientError::Serialize(_)),
        "expected Serialize, got {err:?}"
    );
}
