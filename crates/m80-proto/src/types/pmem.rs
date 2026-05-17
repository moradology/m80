//! Pmem mount payloads carried over the host↔guest wire.

use crate::error::ProtoError;
use crate::types::Payload;
use crate::wire::generated::{
    WirePmemMountRequest, WirePmemMountResponse, WirePmemMountSpec, WirePmemMountStatus,
};
use crate::wire::WirePayload;

/// Wire `kind` for [`PmemMountRequest`].
pub const PAYLOAD_KIND_PMEM_MOUNT_REQUEST: &str = "pmem_mount_request";
/// Wire `kind` for [`PmemMountResponse`].
pub const PAYLOAD_KIND_PMEM_MOUNT_RESPONSE: &str = "pmem_mount_response";

/// One pmem device the host asks the guest to mount.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PmemMountSpec {
    /// Guest block-device path, e.g. `/dev/pmem0`.
    pub device_path: String,
    /// Guest mount path, e.g. `/opt/m80-layers/rust`.
    pub mount_path: String,
    /// Opaque erofs artifact digest. The guest does not verify it.
    pub digest_hex: String,
}

/// Host request for guest pmem mounts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PmemMountRequest {
    /// Devices to mount. Each device receives its own status in the response.
    pub devices: Vec<PmemMountSpec>,
}

/// Per-device pmem mount result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PmemMountStatusKind {
    /// Device was mounted during this request.
    Mounted,
    /// Requested device was already mounted at the requested path.
    AlreadyMounted,
    /// Device could not be mounted.
    Failed,
}

/// Bounded pmem mount failure vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PmemMountError {
    /// Guest did not observe the expected pmem device before timeout.
    DeviceNotFound,
    /// Guest observed the device but mount failed.
    MountFailed,
    /// Guest mounted the device but could not confirm DAX in `/proc/mounts`.
    DaxFlagAbsent,
    /// Device path was not `/dev/pmem<N>`.
    InvalidDevicePath,
    /// Mount path was not an accepted absolute pmem layer path.
    InvalidMountPath,
    /// I/O failure that does not fit a narrower variant.
    Io,
}

/// Per-device pmem mount result with optional error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PmemMountStatus {
    /// Guest block-device path.
    pub device_path: String,
    /// Guest mount path.
    pub mount_path: String,
    /// Mount outcome.
    pub status: PmemMountStatusKind,
    /// Error discriminant; `None` on success.
    pub error: Option<PmemMountError>,
}

/// Guest response to [`PmemMountRequest`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PmemMountResponse {
    /// Per-device mount outcomes. Multiple statuses allow partial success.
    pub statuses: Vec<PmemMountStatus>,
}

fn status_to_i32(status: PmemMountStatusKind) -> i32 {
    match status {
        PmemMountStatusKind::Mounted => 0,
        PmemMountStatusKind::AlreadyMounted => 1,
        PmemMountStatusKind::Failed => 2,
    }
}

fn status_from_i32(value: i32) -> Result<PmemMountStatusKind, ProtoError> {
    match value {
        0 => Ok(PmemMountStatusKind::Mounted),
        1 => Ok(PmemMountStatusKind::AlreadyMounted),
        2 => Ok(PmemMountStatusKind::Failed),
        _ => Err(ProtoError::MalformedPayload(format!(
            "unknown pmem mount status: {value}"
        ))),
    }
}

fn error_to_i32(error: PmemMountError) -> i32 {
    match error {
        PmemMountError::DeviceNotFound => 0,
        PmemMountError::MountFailed => 1,
        PmemMountError::DaxFlagAbsent => 2,
        PmemMountError::InvalidDevicePath => 3,
        PmemMountError::InvalidMountPath => 4,
        PmemMountError::Io => 5,
    }
}

fn error_from_i32(value: i32) -> Result<PmemMountError, ProtoError> {
    match value {
        0 => Ok(PmemMountError::DeviceNotFound),
        1 => Ok(PmemMountError::MountFailed),
        2 => Ok(PmemMountError::DaxFlagAbsent),
        3 => Ok(PmemMountError::InvalidDevicePath),
        4 => Ok(PmemMountError::InvalidMountPath),
        5 => Ok(PmemMountError::Io),
        _ => Err(ProtoError::MalformedPayload(format!(
            "unknown pmem mount error: {value}"
        ))),
    }
}

fn opt_error_from_i32(value: Option<i32>) -> Result<Option<PmemMountError>, ProtoError> {
    value.map(error_from_i32).transpose()
}

fn opt_error_to_i32(value: Option<PmemMountError>) -> Option<i32> {
    value.map(error_to_i32)
}

fn spec_to_wire(spec: PmemMountSpec) -> WirePmemMountSpec {
    WirePmemMountSpec {
        device_path: spec.device_path,
        mount_path: spec.mount_path,
        digest_hex: spec.digest_hex,
    }
}

fn spec_from_wire(spec: WirePmemMountSpec) -> PmemMountSpec {
    PmemMountSpec {
        device_path: spec.device_path,
        mount_path: spec.mount_path,
        digest_hex: spec.digest_hex,
    }
}

fn status_to_wire(status: PmemMountStatus) -> WirePmemMountStatus {
    WirePmemMountStatus {
        device_path: status.device_path,
        mount_path: status.mount_path,
        status: status_to_i32(status.status),
        error: opt_error_to_i32(status.error),
    }
}

fn status_from_wire(status: WirePmemMountStatus) -> Result<PmemMountStatus, ProtoError> {
    Ok(PmemMountStatus {
        device_path: status.device_path,
        mount_path: status.mount_path,
        status: status_from_i32(status.status)?,
        error: opt_error_from_i32(status.error)?,
    })
}

impl Payload for PmemMountRequest {
    const KIND: &'static str = PAYLOAD_KIND_PMEM_MOUNT_REQUEST;

    fn into_wire(self) -> WirePayload {
        WirePayload::PmemMountRequest(WirePmemMountRequest {
            devices: self.devices.into_iter().map(spec_to_wire).collect(),
        })
    }

    fn from_wire(payload: WirePayload) -> Result<Self, ProtoError> {
        match payload {
            WirePayload::PmemMountRequest(value) => Ok(Self {
                devices: value.devices.into_iter().map(spec_from_wire).collect(),
            }),
            _ => Err(ProtoError::MalformedPayload(
                "unexpected protobuf payload for pmem_mount_request".into(),
            )),
        }
    }
}

impl Payload for PmemMountResponse {
    const KIND: &'static str = PAYLOAD_KIND_PMEM_MOUNT_RESPONSE;

    fn into_wire(self) -> WirePayload {
        WirePayload::PmemMountResponse(WirePmemMountResponse {
            statuses: self.statuses.into_iter().map(status_to_wire).collect(),
        })
    }

    fn from_wire(payload: WirePayload) -> Result<Self, ProtoError> {
        match payload {
            WirePayload::PmemMountResponse(value) => Ok(Self {
                statuses: value
                    .statuses
                    .into_iter()
                    .map(status_from_wire)
                    .collect::<Result<_, _>>()?,
            }),
            _ => Err(ProtoError::MalformedPayload(
                "unexpected protobuf payload for pmem_mount_response".into(),
            )),
        }
    }
}
