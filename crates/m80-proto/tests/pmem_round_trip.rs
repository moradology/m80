use std::io::Cursor;

use m80_proto::{
    read_frame, write_frame, Envelope, PmemMountError, PmemMountRequest, PmemMountResponse,
    PmemMountSpec, PmemMountStatus, PmemMountStatusKind,
};

fn round_trip<T>(payload: T) -> Envelope<T>
where
    T: m80_proto::Payload + Clone,
{
    let env = Envelope::with_request_id(payload, "req-pmem".to_owned());
    let mut bytes = Vec::new();
    write_frame(&mut bytes, &env).unwrap();
    read_frame(&mut Cursor::new(bytes)).unwrap()
}

#[test]
fn pmem_mount_request_round_trips_specs() {
    let decoded = round_trip(PmemMountRequest {
        devices: vec![PmemMountSpec {
            device_path: "/dev/pmem0".to_owned(),
            mount_path: "/opt/m80-layers/rust".to_owned(),
            digest_hex: "a".repeat(64),
        }],
    });

    assert_eq!(decoded.request_id.as_deref(), Some("req-pmem"));
    assert_eq!(decoded.payload.devices[0].device_path, "/dev/pmem0");
    assert_eq!(
        decoded.payload.devices[0].mount_path,
        "/opt/m80-layers/rust"
    );
    assert_eq!(decoded.payload.devices[0].digest_hex, "a".repeat(64));
}

#[test]
fn pmem_mount_response_round_trips_statuses() {
    let decoded = round_trip(PmemMountResponse {
        statuses: vec![
            PmemMountStatus {
                device_path: "/dev/pmem0".to_owned(),
                mount_path: "/opt/m80-layers/rust".to_owned(),
                status: PmemMountStatusKind::Mounted,
                error: None,
            },
            PmemMountStatus {
                device_path: "/dev/pmem1".to_owned(),
                mount_path: "/opt/m80-layers/node".to_owned(),
                status: PmemMountStatusKind::Failed,
                error: Some(PmemMountError::DaxFlagAbsent),
            },
        ],
    });

    assert_eq!(decoded.payload.statuses.len(), 2);
    assert_eq!(
        decoded.payload.statuses[0].status,
        PmemMountStatusKind::Mounted
    );
    assert_eq!(decoded.payload.statuses[0].error, None);
    assert_eq!(
        decoded.payload.statuses[1].error,
        Some(PmemMountError::DaxFlagAbsent)
    );
}
