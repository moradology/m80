//! Drive hotplug and tenant-identity payloads carried over the host↔guest wire.

use crate::error::ProtoError;
use crate::types::Payload;
use crate::wire::generated::{
    WireDriveDetachRequest, WireDriveDetachResponse, WireDriveDetachSpec, WireDriveDetachStatus,
    WireDriveMountRequest, WireDriveMountResponse, WireDriveMountSpec, WireDriveMountStatus,
    WireTenantIdentityReport,
};
use crate::wire::{WirePayload, WirePayload::*};

/// Wire `kind` for [`DriveMountRequest`].
pub const PAYLOAD_KIND_DRIVE_MOUNT_REQUEST: &str = "drive_mount_request";
/// Wire `kind` for [`DriveMountResponse`].
pub const PAYLOAD_KIND_DRIVE_MOUNT_RESPONSE: &str = "drive_mount_response";
/// Wire `kind` for [`DriveDetachRequest`].
pub const PAYLOAD_KIND_DRIVE_DETACH_REQUEST: &str = "drive_detach_request";
/// Wire `kind` for [`DriveDetachResponse`].
pub const PAYLOAD_KIND_DRIVE_DETACH_RESPONSE: &str = "drive_detach_response";

/// One drive the host asks the guest to discover and mount.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriveMountSpec {
    /// Firecracker drive id, e.g. `hotplug_slot_0`.
    pub drive_id: String,
    /// Guest block-device path, e.g. `/dev/vdd`.
    pub device_path: String,
    /// Guest mount path, e.g. `/workspace`.
    pub mount_path: String,
    /// Optional guest path to read for opaque tenant identity bytes.
    pub identity_path: Option<String>,
}

/// Host request for one or more guest drive mounts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriveMountRequest {
    /// Devices to mount. Each device receives its own status in the response.
    pub devices: Vec<DriveMountSpec>,
}

/// Per-device mount result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriveMountStatusKind {
    /// Device was mounted during this request.
    Mounted,
    /// Requested device was already mounted at the requested path.
    AlreadyMounted,
    /// Device could not be mounted.
    Failed,
}

/// Per-device detach result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriveDetachStatusKind {
    /// Guest unmounted or ejected the device during this request.
    Detached,
    /// Requested device was not mounted.
    NotMounted,
    /// Guest could not unmount or eject the device.
    Failed,
}

/// Bounded drive hotplug failure vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriveHotplugError {
    /// Guest did not observe the expected block device before timeout.
    DeviceNotFound,
    /// Guest observed the device but mount failed.
    MountFailed,
    /// Guest failed to unmount or flush the mounted device.
    UnmountFailed,
    /// Requested identity file was absent.
    IdentityMissing,
    /// Identity bytes could not be read.
    IdentityReadFailed,
    /// Operation exceeded its bounded wait.
    Timeout,
    /// I/O failure that does not fit a narrower variant.
    Io,
}

/// Opaque identity bytes read by the guest from a mounted drive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TenantIdentityReport {
    /// Firecracker drive id whose mounted filesystem produced the identity.
    pub drive_id: String,
    /// Guest path that was read.
    pub path: String,
    /// Opaque identity bytes. m80 does not parse or validate these bytes.
    pub bytes: Vec<u8>,
}

/// Per-device mount result with optional error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriveMountStatus {
    /// Firecracker drive id.
    pub drive_id: String,
    /// Guest mount path.
    pub mount_path: String,
    /// Mount outcome.
    pub status: DriveMountStatusKind,
    /// Error discriminant; `None` on success.
    pub error: Option<DriveHotplugError>,
}

/// Guest response to [`DriveMountRequest`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriveMountResponse {
    /// Per-device mount outcomes. Multiple statuses allow partial success.
    pub statuses: Vec<DriveMountStatus>,
    /// Opaque identity bytes read from mounted drives.
    pub identities: Vec<TenantIdentityReport>,
}

/// One drive the host asks the guest to unmount/eject.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriveDetachSpec {
    /// Firecracker drive id.
    pub drive_id: String,
    /// Guest mount path.
    pub mount_path: String,
}

/// Host request for one or more guest drive detach operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriveDetachRequest {
    /// Devices to detach. Each device receives its own status in the response.
    pub devices: Vec<DriveDetachSpec>,
}

/// Per-device detach result with optional error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriveDetachStatus {
    /// Firecracker drive id.
    pub drive_id: String,
    /// Guest mount path.
    pub mount_path: String,
    /// Detach outcome.
    pub status: DriveDetachStatusKind,
    /// Error discriminant; `None` on success.
    pub error: Option<DriveHotplugError>,
}

/// Guest response to [`DriveDetachRequest`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriveDetachResponse {
    /// Per-device detach outcomes. Multiple statuses allow partial success.
    pub statuses: Vec<DriveDetachStatus>,
}

fn mount_status_to_i32(status: DriveMountStatusKind) -> i32 {
    match status {
        DriveMountStatusKind::Mounted => 0,
        DriveMountStatusKind::AlreadyMounted => 1,
        DriveMountStatusKind::Failed => 2,
    }
}

fn mount_status_from_i32(value: i32) -> Result<DriveMountStatusKind, ProtoError> {
    match value {
        0 => Ok(DriveMountStatusKind::Mounted),
        1 => Ok(DriveMountStatusKind::AlreadyMounted),
        2 => Ok(DriveMountStatusKind::Failed),
        _ => Err(ProtoError::MalformedPayload(format!(
            "unknown drive mount status: {value}"
        ))),
    }
}

fn detach_status_to_i32(status: DriveDetachStatusKind) -> i32 {
    match status {
        DriveDetachStatusKind::Detached => 0,
        DriveDetachStatusKind::NotMounted => 1,
        DriveDetachStatusKind::Failed => 2,
    }
}

fn detach_status_from_i32(value: i32) -> Result<DriveDetachStatusKind, ProtoError> {
    match value {
        0 => Ok(DriveDetachStatusKind::Detached),
        1 => Ok(DriveDetachStatusKind::NotMounted),
        2 => Ok(DriveDetachStatusKind::Failed),
        _ => Err(ProtoError::MalformedPayload(format!(
            "unknown drive detach status: {value}"
        ))),
    }
}

fn hotplug_error_to_i32(error: DriveHotplugError) -> i32 {
    match error {
        DriveHotplugError::DeviceNotFound => 0,
        DriveHotplugError::MountFailed => 1,
        DriveHotplugError::UnmountFailed => 2,
        DriveHotplugError::IdentityMissing => 3,
        DriveHotplugError::IdentityReadFailed => 4,
        DriveHotplugError::Timeout => 5,
        DriveHotplugError::Io => 6,
    }
}

fn hotplug_error_from_i32(value: i32) -> Result<DriveHotplugError, ProtoError> {
    match value {
        0 => Ok(DriveHotplugError::DeviceNotFound),
        1 => Ok(DriveHotplugError::MountFailed),
        2 => Ok(DriveHotplugError::UnmountFailed),
        3 => Ok(DriveHotplugError::IdentityMissing),
        4 => Ok(DriveHotplugError::IdentityReadFailed),
        5 => Ok(DriveHotplugError::Timeout),
        6 => Ok(DriveHotplugError::Io),
        _ => Err(ProtoError::MalformedPayload(format!(
            "unknown drive hotplug error: {value}"
        ))),
    }
}

fn opt_hotplug_error_from_i32(value: Option<i32>) -> Result<Option<DriveHotplugError>, ProtoError> {
    value.map(hotplug_error_from_i32).transpose()
}

fn opt_hotplug_error_to_i32(value: Option<DriveHotplugError>) -> Option<i32> {
    value.map(hotplug_error_to_i32)
}

fn mount_spec_to_wire(spec: DriveMountSpec) -> WireDriveMountSpec {
    WireDriveMountSpec {
        drive_id: spec.drive_id,
        device_path: spec.device_path,
        mount_path: spec.mount_path,
        identity_path: spec.identity_path,
    }
}

fn mount_spec_from_wire(spec: WireDriveMountSpec) -> DriveMountSpec {
    DriveMountSpec {
        drive_id: spec.drive_id,
        device_path: spec.device_path,
        mount_path: spec.mount_path,
        identity_path: spec.identity_path,
    }
}

fn mount_status_to_wire(status: DriveMountStatus) -> WireDriveMountStatus {
    WireDriveMountStatus {
        drive_id: status.drive_id,
        mount_path: status.mount_path,
        status: mount_status_to_i32(status.status),
        error: opt_hotplug_error_to_i32(status.error),
    }
}

fn mount_status_from_wire(status: WireDriveMountStatus) -> Result<DriveMountStatus, ProtoError> {
    Ok(DriveMountStatus {
        drive_id: status.drive_id,
        mount_path: status.mount_path,
        status: mount_status_from_i32(status.status)?,
        error: opt_hotplug_error_from_i32(status.error)?,
    })
}

fn identity_to_wire(identity: TenantIdentityReport) -> WireTenantIdentityReport {
    WireTenantIdentityReport {
        drive_id: identity.drive_id,
        path: identity.path,
        bytes: identity.bytes,
    }
}

fn identity_from_wire(identity: WireTenantIdentityReport) -> TenantIdentityReport {
    TenantIdentityReport {
        drive_id: identity.drive_id,
        path: identity.path,
        bytes: identity.bytes,
    }
}

fn detach_spec_to_wire(spec: DriveDetachSpec) -> WireDriveDetachSpec {
    WireDriveDetachSpec {
        drive_id: spec.drive_id,
        mount_path: spec.mount_path,
    }
}

fn detach_spec_from_wire(spec: WireDriveDetachSpec) -> DriveDetachSpec {
    DriveDetachSpec {
        drive_id: spec.drive_id,
        mount_path: spec.mount_path,
    }
}

fn detach_status_to_wire(status: DriveDetachStatus) -> WireDriveDetachStatus {
    WireDriveDetachStatus {
        drive_id: status.drive_id,
        mount_path: status.mount_path,
        status: detach_status_to_i32(status.status),
        error: opt_hotplug_error_to_i32(status.error),
    }
}

fn detach_status_from_wire(status: WireDriveDetachStatus) -> Result<DriveDetachStatus, ProtoError> {
    Ok(DriveDetachStatus {
        drive_id: status.drive_id,
        mount_path: status.mount_path,
        status: detach_status_from_i32(status.status)?,
        error: opt_hotplug_error_from_i32(status.error)?,
    })
}

impl Payload for DriveMountRequest {
    const KIND: &'static str = PAYLOAD_KIND_DRIVE_MOUNT_REQUEST;

    fn into_wire(self) -> WirePayload {
        DriveMountRequest(WireDriveMountRequest {
            devices: self.devices.into_iter().map(mount_spec_to_wire).collect(),
        })
    }

    fn from_wire(payload: WirePayload) -> Result<Self, ProtoError> {
        match payload {
            DriveMountRequest(value) => Ok(Self {
                devices: value
                    .devices
                    .into_iter()
                    .map(mount_spec_from_wire)
                    .collect(),
            }),
            _ => Err(ProtoError::MalformedPayload(
                "unexpected protobuf payload for drive_mount_request".into(),
            )),
        }
    }
}

impl Payload for DriveMountResponse {
    const KIND: &'static str = PAYLOAD_KIND_DRIVE_MOUNT_RESPONSE;

    fn into_wire(self) -> WirePayload {
        DriveMountResponse(WireDriveMountResponse {
            statuses: self
                .statuses
                .into_iter()
                .map(mount_status_to_wire)
                .collect(),
            identities: self.identities.into_iter().map(identity_to_wire).collect(),
        })
    }

    fn from_wire(payload: WirePayload) -> Result<Self, ProtoError> {
        match payload {
            DriveMountResponse(value) => Ok(Self {
                statuses: value
                    .statuses
                    .into_iter()
                    .map(mount_status_from_wire)
                    .collect::<Result<_, _>>()?,
                identities: value
                    .identities
                    .into_iter()
                    .map(identity_from_wire)
                    .collect(),
            }),
            _ => Err(ProtoError::MalformedPayload(
                "unexpected protobuf payload for drive_mount_response".into(),
            )),
        }
    }
}

impl Payload for DriveDetachRequest {
    const KIND: &'static str = PAYLOAD_KIND_DRIVE_DETACH_REQUEST;

    fn into_wire(self) -> WirePayload {
        DriveDetachRequest(WireDriveDetachRequest {
            devices: self.devices.into_iter().map(detach_spec_to_wire).collect(),
        })
    }

    fn from_wire(payload: WirePayload) -> Result<Self, ProtoError> {
        match payload {
            DriveDetachRequest(value) => Ok(Self {
                devices: value
                    .devices
                    .into_iter()
                    .map(detach_spec_from_wire)
                    .collect(),
            }),
            _ => Err(ProtoError::MalformedPayload(
                "unexpected protobuf payload for drive_detach_request".into(),
            )),
        }
    }
}

impl Payload for DriveDetachResponse {
    const KIND: &'static str = PAYLOAD_KIND_DRIVE_DETACH_RESPONSE;

    fn into_wire(self) -> WirePayload {
        DriveDetachResponse(WireDriveDetachResponse {
            statuses: self
                .statuses
                .into_iter()
                .map(detach_status_to_wire)
                .collect(),
        })
    }

    fn from_wire(payload: WirePayload) -> Result<Self, ProtoError> {
        match payload {
            DriveDetachResponse(value) => Ok(Self {
                statuses: value
                    .statuses
                    .into_iter()
                    .map(detach_status_from_wire)
                    .collect::<Result<_, _>>()?,
            }),
            _ => Err(ProtoError::MalformedPayload(
                "unexpected protobuf payload for drive_detach_response".into(),
            )),
        }
    }
}
