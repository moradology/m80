use std::path::PathBuf;

use super::*;
use m80_proto::{DriveDetachStatus, DriveMountStatus, DriveMountStatusKind};

fn attach_request(expected_identity: &[u8]) -> HotplugDriveAttach {
    HotplugDriveAttach {
        slot: 0,
        path_on_host: PathBuf::from("/tenant.ext4"),
        mount_path: "/workspace".into(),
        identity_path: "/workspace/.tenant-identity".into(),
        expected_identity: expected_identity.to_vec(),
    }
}

fn detach_request() -> HotplugDriveDetach {
    HotplugDriveDetach {
        slot: 0,
        mount_path: "/workspace".into(),
    }
}

fn mounted_response(bytes: &[u8]) -> DriveMountResponse {
    DriveMountResponse {
        statuses: vec![DriveMountStatus {
            drive_id: "hotplug_slot_0".into(),
            mount_path: "/workspace".into(),
            status: DriveMountStatusKind::Mounted,
            error: None,
        }],
        identities: vec![TenantIdentityReport {
            drive_id: "hotplug_slot_0".into(),
            path: "/workspace/.tenant-identity".into(),
            bytes: bytes.to_vec(),
        }],
    }
}

fn detached_response(status: DriveDetachStatusKind) -> DriveDetachResponse {
    DriveDetachResponse {
        statuses: vec![DriveDetachStatus {
            drive_id: "hotplug_slot_0".into(),
            mount_path: "/workspace".into(),
            status,
            error: None,
        }],
    }
}

#[test]
fn slot_device_path_without_workspace_starts_at_vdc() {
    assert_eq!(preallocated_drive_device_path(false, 0), "/dev/vdc");
    assert_eq!(preallocated_drive_device_path(false, 1), "/dev/vdd");
}

#[test]
fn slot_device_path_with_workspace_starts_at_vdd() {
    assert_eq!(preallocated_drive_device_path(true, 0), "/dev/vdd");
    assert_eq!(preallocated_drive_device_path(true, 1), "/dev/vde");
}

#[test]
fn slot_device_path_supports_post_z_suffixes() {
    // bijective base-26 boundaries (with workspace=false baseline index=2):
    //   slot 24 → index 26 → "aa" (first two-letter suffix)
    //   slot 25 → index 27 → "ab"
    //   slot 50 → index 52 → "ba"
    //   slot 51 → index 53 → "bb"
    assert_eq!(preallocated_drive_device_path(false, 24), "/dev/vdaa");
    assert_eq!(preallocated_drive_device_path(false, 25), "/dev/vdab");
    assert_eq!(preallocated_drive_device_path(false, 50), "/dev/vdba");
    assert_eq!(preallocated_drive_device_path(false, 51), "/dev/vdbb");
}

#[test]
fn mount_response_accepts_matching_identity() {
    validate_mount_response(
        &mounted_response(b"tenant-a"),
        &attach_request(b"tenant-a"),
        "hotplug_slot_0",
    )
    .unwrap();
}

#[test]
fn mount_response_accepts_already_mounted_matching_identity() {
    let mut response = mounted_response(b"tenant-a");
    response.statuses[0].status = DriveMountStatusKind::AlreadyMounted;

    validate_mount_response(&response, &attach_request(b"tenant-a"), "hotplug_slot_0").unwrap();
}

#[test]
fn mount_response_rejects_missing_requested_status() {
    let err = validate_mount_response(
        &mounted_response(b"tenant-a"),
        &attach_request(b"tenant-a"),
        "hotplug_slot_1",
    )
    .unwrap_err();

    assert!(matches!(
        err,
        FcError::Protocol(crate::error::WireProtocolError::UnexpectedFrame {
            context: "drive mount",
            expected: "status for requested drive",
            ..
        })
    ));
}

#[test]
fn mount_response_rejects_missing_identity_report() {
    let response = DriveMountResponse {
        statuses: vec![DriveMountStatus {
            drive_id: "hotplug_slot_0".into(),
            mount_path: "/workspace".into(),
            status: DriveMountStatusKind::Mounted,
            error: None,
        }],
        identities: vec![],
    };

    let err = validate_mount_response(&response, &attach_request(b"tenant-a"), "hotplug_slot_0")
        .unwrap_err();

    assert!(matches!(
        err,
        FcError::DriveHotplug(DriveHotplugError::IdentityMissing)
    ));
}

#[test]
fn mount_response_rejects_identity_mismatch() {
    let err = validate_mount_response(
        &mounted_response(b"tenant-b"),
        &attach_request(b"tenant-a"),
        "hotplug_slot_0",
    )
    .unwrap_err();

    assert!(matches!(
        err,
        FcError::TenantIdentityMismatch {
            drive_id,
            expected_len: 8,
            actual_len: 8,
        } if drive_id == "hotplug_slot_0"
    ));
}

#[test]
fn mount_response_rejects_failed_guest_status() {
    let response = DriveMountResponse {
        statuses: vec![DriveMountStatus {
            drive_id: "hotplug_slot_0".into(),
            mount_path: "/workspace".into(),
            status: DriveMountStatusKind::Failed,
            error: Some(DriveHotplugError::Timeout),
        }],
        identities: vec![],
    };

    let err = validate_mount_response(&response, &attach_request(b"tenant-a"), "hotplug_slot_0")
        .unwrap_err();

    assert!(matches!(
        err,
        FcError::DriveHotplug(DriveHotplugError::Timeout)
    ));
}

#[test]
fn detach_response_accepts_detached_status() {
    validate_detach_response(
        &detached_response(DriveDetachStatusKind::Detached),
        "hotplug_slot_0",
    )
    .unwrap();
}

#[test]
fn detach_response_accepts_not_mounted_status() {
    validate_detach_response(
        &detached_response(DriveDetachStatusKind::NotMounted),
        "hotplug_slot_0",
    )
    .unwrap();
}

#[test]
fn detach_response_rejects_failed_guest_status() {
    let mut response = detached_response(DriveDetachStatusKind::Failed);
    response.statuses[0].error = Some(DriveHotplugError::Timeout);

    let err = validate_detach_response(&response, "hotplug_slot_0").unwrap_err();

    assert!(matches!(
        err,
        FcError::DriveHotplug(DriveHotplugError::Timeout)
    ));
}

#[test]
fn detach_request_rejects_empty_mount_path() {
    let request = HotplugDriveDetach {
        mount_path: String::new(),
        ..detach_request()
    };

    let err = validate_detach_request(&request).unwrap_err();

    assert!(matches!(
        err,
        FcError::Config(ConfigError::InvalidValue {
            field: "mount_path",
            ..
        })
    ));
}

#[test]
fn attach_request_rejects_relative_firecracker_path() {
    let request = HotplugDriveAttach {
        path_on_host: PathBuf::from("tenant.ext4"),
        ..attach_request(b"tenant-a")
    };

    let err = validate_attach_request(&request).unwrap_err();

    assert!(matches!(
        err,
        FcError::Config(ConfigError::InvalidValue {
            field: "path_on_host",
            ..
        })
    ));
}
