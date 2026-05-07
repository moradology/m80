//! Host-side hotplug methods for [`RunningSandbox`].

use std::sync::atomic::Ordering;
use std::time::Instant;

use m80_firecracker_client::PartialDriveConfig;
use m80_proto::{
    DriveHotplugError, DriveMountRequest, DriveMountResponse, DriveMountSpec, DriveMountStatusKind,
    Envelope, TenantIdentityReport,
};

use crate::diagnostics::phase_event;
use crate::error::{ConfigError, FcError};
use crate::hotplug_types::HotplugDriveAttach;
use crate::lifecycle::exec::{request_id_for, send_envelope_with_open_retry};
use crate::lifecycle::monotonic_ns;
use crate::preboot::preallocated_drive_slot_id;
use crate::types::RunningSandbox;

impl RunningSandbox {
    /// Attach one preallocated drive slot, ask guestd to mount it, and return
    /// only after the guest reports the expected opaque identity bytes.
    ///
    /// Failures consume the sandbox and discard the VM, because a partially
    /// attached tenant drive is not a reusable running state.
    pub fn attach_drive_verified(mut self, request: HotplugDriveAttach) -> Result<Self, FcError> {
        match self.attach_drive_verified_inner(&request) {
            Ok(()) => Ok(self),
            Err(err) => Err(discard_after_attach_failure(self, err)),
        }
    }

    fn attach_drive_verified_inner(&mut self, request: &HotplugDriveAttach) -> Result<(), FcError> {
        self.prepare_hotplug_activity()?;
        validate_attach_request(request)?;
        if request.slot >= self.preallocated_drive_slots {
            return Err(FcError::Config(ConfigError::InvalidValue {
                field: "slot",
                reason: format!(
                    "slot {} is outside {} preallocated drive slots",
                    request.slot, self.preallocated_drive_slots
                ),
            }));
        }

        let drive_id = preallocated_drive_slot_id(request.slot);
        let device_path = preallocated_drive_device_path(self.scratch.is_some(), request.slot);
        let patch = PartialDriveConfig {
            drive_id: drive_id.clone(),
            path_on_host: Some(request.path_on_host.clone()),
        };
        let t_patch = Instant::now();
        self.client.patch_drive(&patch)?;
        phase_event("hotplug_drive_patch", &self.vm_id, t_patch.elapsed());

        let vsock_uds = self.jail.jail_path.join("vsock.sock");
        let request_id = request_id_for(&self.vm_id, self.request_id.as_deref(), "drive_mount");
        let envelope = Envelope::with_request_id(
            DriveMountRequest {
                devices: vec![DriveMountSpec {
                    drive_id: drive_id.clone(),
                    device_path,
                    mount_path: request.mount_path.clone(),
                    identity_path: Some(request.identity_path.clone()),
                }],
            },
            request_id,
        );
        let t_mount = Instant::now();
        let mut channel = send_envelope_with_open_retry(&vsock_uds, &self.vm_id, &envelope)?;
        let response: Envelope<DriveMountResponse> = channel
            .recv()
            .map_err(|e| super::protocol::recv_error(e, "drive mount"))?;
        validate_mount_response(&response.payload, request, &drive_id)?;
        phase_event("hotplug_drive_mount", &self.vm_id, t_mount.elapsed());
        self.last_activity_ns
            .store(monotonic_ns(), Ordering::Relaxed);
        Ok(())
    }

    fn prepare_hotplug_activity(&self) -> Result<(), FcError> {
        if self.idle_timed_out.load(Ordering::Relaxed) {
            return Err(FcError::IdleTimedOut);
        }
        self.last_activity_ns
            .store(monotonic_ns(), Ordering::Relaxed);
        Ok(())
    }
}

fn validate_attach_request(request: &HotplugDriveAttach) -> Result<(), FcError> {
    if !request.path_on_host.is_absolute() {
        return Err(FcError::Config(ConfigError::InvalidValue {
            field: "path_on_host",
            reason: "must be absolute and visible inside the Firecracker jail".into(),
        }));
    }
    if request.mount_path.is_empty() {
        return Err(FcError::Config(ConfigError::InvalidValue {
            field: "mount_path",
            reason: "must not be empty".into(),
        }));
    }
    if request.identity_path.is_empty() {
        return Err(FcError::Config(ConfigError::InvalidValue {
            field: "identity_path",
            reason: "must not be empty".into(),
        }));
    }
    Ok(())
}

fn validate_mount_response(
    response: &DriveMountResponse,
    request: &HotplugDriveAttach,
    drive_id: &str,
) -> Result<(), FcError> {
    let status = response
        .statuses
        .iter()
        .find(|status| status.drive_id == drive_id)
        .ok_or_else(|| {
            super::protocol::unexpected_frame(
                "drive mount",
                "status for requested drive",
                "missing",
            )
        })?;
    match status.status {
        DriveMountStatusKind::Mounted | DriveMountStatusKind::AlreadyMounted => {}
        DriveMountStatusKind::Failed => {
            return Err(FcError::DriveHotplug(
                status.error.unwrap_or(DriveHotplugError::Io),
            ));
        }
    }

    let identity = response
        .identities
        .iter()
        .find(|identity| identity.drive_id == drive_id && identity.path == request.identity_path)
        .ok_or(FcError::DriveHotplug(DriveHotplugError::IdentityMissing))?;
    verify_identity(drive_id, &request.expected_identity, identity)
}

fn verify_identity(
    drive_id: &str,
    expected: &[u8],
    actual: &TenantIdentityReport,
) -> Result<(), FcError> {
    if actual.bytes == expected {
        return Ok(());
    }
    Err(FcError::TenantIdentityMismatch {
        drive_id: drive_id.to_owned(),
        expected_len: expected.len(),
        actual_len: actual.bytes.len(),
    })
}

fn preallocated_drive_device_path(has_workspace: bool, slot: u8) -> String {
    let index = 2 + u16::from(has_workspace) + u16::from(slot);
    format!("/dev/vd{}", virtio_blk_suffix(index))
}

fn virtio_blk_suffix(mut index: u16) -> String {
    let mut suffix = Vec::new();
    loop {
        suffix.push((b'a' + (index % 26) as u8) as char);
        if index < 26 {
            break;
        }
        index = index / 26 - 1;
    }
    suffix.iter().rev().collect()
}

fn discard_after_attach_failure(sandbox: RunningSandbox, err: FcError) -> FcError {
    if let Err(cleanup) = sandbox.force_kill().and_then(|stopped| stopped.delete()) {
        tracing::warn!(
            error = %cleanup,
            "failed to discard sandbox after drive attach failure"
        );
    }
    err
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use m80_proto::{DriveMountStatus, DriveMountStatusKind};

    fn attach_request(expected_identity: &[u8]) -> HotplugDriveAttach {
        HotplugDriveAttach {
            slot: 0,
            path_on_host: PathBuf::from("/tenant.ext4"),
            mount_path: "/workspace".into(),
            identity_path: "/workspace/.tenant-identity".into(),
            expected_identity: expected_identity.to_vec(),
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
        assert_eq!(preallocated_drive_device_path(false, 24), "/dev/vdaa");
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

        let err =
            validate_mount_response(&response, &attach_request(b"tenant-a"), "hotplug_slot_0")
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

        let err =
            validate_mount_response(&response, &attach_request(b"tenant-a"), "hotplug_slot_0")
                .unwrap_err();

        assert!(matches!(
            err,
            FcError::DriveHotplug(DriveHotplugError::Timeout)
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
}
