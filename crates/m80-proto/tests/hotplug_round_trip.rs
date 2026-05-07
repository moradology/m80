use std::io::Cursor;

use m80_proto::{
    read_frame, write_frame, DriveDetachRequest, DriveDetachResponse, DriveDetachSpec,
    DriveDetachStatus, DriveDetachStatusKind, DriveHotplugError, DriveMountRequest,
    DriveMountResponse, DriveMountSpec, DriveMountStatus, DriveMountStatusKind, Envelope,
    TenantIdentityReport,
};

fn round_trip<T>(payload: T) -> Envelope<T>
where
    T: m80_proto::Payload + Clone,
{
    let env = Envelope::with_request_id(payload, "req-hotplug".to_owned());
    let mut bytes = Vec::new();
    write_frame(&mut bytes, &env).unwrap();
    read_frame(&mut Cursor::new(bytes)).unwrap()
}

#[test]
fn drive_mount_request_round_trips_mount_specs() {
    let decoded = round_trip(DriveMountRequest {
        devices: vec![
            DriveMountSpec {
                drive_id: "hotplug_slot_0".to_owned(),
                device_path: "/dev/vdd".to_owned(),
                mount_path: "/workspace".to_owned(),
                identity_path: Some("/workspace/.tenant-identity".to_owned()),
            },
            DriveMountSpec {
                drive_id: "hotplug_slot_1".to_owned(),
                device_path: "/dev/vde".to_owned(),
                mount_path: "/cache".to_owned(),
                identity_path: None,
            },
        ],
    });

    assert_eq!(decoded.request_id.as_deref(), Some("req-hotplug"));
    assert_eq!(decoded.payload.devices[0].drive_id, "hotplug_slot_0");
    assert_eq!(decoded.payload.devices[0].device_path, "/dev/vdd");
    assert_eq!(
        decoded.payload.devices[0].identity_path.as_deref(),
        Some("/workspace/.tenant-identity")
    );
    assert_eq!(decoded.payload.devices[1].mount_path, "/cache");
    assert!(decoded.payload.devices[1].identity_path.is_none());
}

#[test]
fn drive_mount_response_round_trips_partial_success_and_identity_bytes() {
    let decoded = round_trip(DriveMountResponse {
        statuses: vec![
            DriveMountStatus {
                drive_id: "hotplug_slot_0".to_owned(),
                mount_path: "/workspace".to_owned(),
                status: DriveMountStatusKind::Mounted,
                error: None,
            },
            DriveMountStatus {
                drive_id: "hotplug_slot_1".to_owned(),
                mount_path: "/cache".to_owned(),
                status: DriveMountStatusKind::Failed,
                error: Some(DriveHotplugError::Timeout),
            },
        ],
        identities: vec![TenantIdentityReport {
            drive_id: "hotplug_slot_0".to_owned(),
            path: "/workspace/.tenant-identity".to_owned(),
            bytes: b"opaque-token-bytes".to_vec(),
        }],
    });

    assert_eq!(decoded.payload.statuses.len(), 2);
    assert_eq!(
        decoded.payload.statuses[0].status,
        DriveMountStatusKind::Mounted
    );
    assert_eq!(decoded.payload.statuses[0].error, None);
    assert_eq!(
        decoded.payload.statuses[1].error,
        Some(DriveHotplugError::Timeout)
    );
    assert_eq!(decoded.payload.identities[0].bytes, b"opaque-token-bytes");
}

#[test]
fn drive_detach_request_round_trips_devices() {
    let decoded = round_trip(DriveDetachRequest {
        devices: vec![DriveDetachSpec {
            drive_id: "hotplug_slot_0".to_owned(),
            mount_path: "/workspace".to_owned(),
        }],
    });

    assert_eq!(decoded.payload.devices[0].drive_id, "hotplug_slot_0");
    assert_eq!(decoded.payload.devices[0].mount_path, "/workspace");
}

#[test]
fn drive_detach_response_round_trips_partial_success() {
    let decoded = round_trip(DriveDetachResponse {
        statuses: vec![
            DriveDetachStatus {
                drive_id: "hotplug_slot_0".to_owned(),
                mount_path: "/workspace".to_owned(),
                status: DriveDetachStatusKind::Detached,
                error: None,
            },
            DriveDetachStatus {
                drive_id: "hotplug_slot_1".to_owned(),
                mount_path: "/cache".to_owned(),
                status: DriveDetachStatusKind::Failed,
                error: Some(DriveHotplugError::UnmountFailed),
            },
        ],
    });

    assert_eq!(
        decoded.payload.statuses[0].status,
        DriveDetachStatusKind::Detached
    );
    assert_eq!(
        decoded.payload.statuses[1].error,
        Some(DriveHotplugError::UnmountFailed)
    );
}
