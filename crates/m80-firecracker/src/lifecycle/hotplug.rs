//! Host-side hotplug methods for [`RunningSandbox`].

use std::sync::atomic::Ordering;
use std::time::Instant;

use m80_firecracker_client::PartialDriveConfig;
use m80_proto::{
    DriveDetachRequest, DriveDetachResponse, DriveDetachSpec, DriveDetachStatusKind,
    DriveHotplugError, DriveMountRequest, DriveMountResponse, DriveMountSpec, DriveMountStatusKind,
    Envelope, TenantIdentityReport,
};

use crate::diagnostics::phase_event;
use crate::error::{ConfigError, FcError};
use crate::hotplug_types::{HotplugDriveAttach, HotplugDriveDetach};
use crate::layout::{preallocated_drive_slot_jail_path, VSOCK_SOCKET};
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
            Err(err) => Err(discard_after_hotplug_failure(self, err)),
        }
    }

    /// Ask guestd to unmount one hotplug slot, then retarget that Firecracker
    /// slot back to its placeholder backing file before returning the VM.
    pub fn detach_drive(mut self, request: HotplugDriveDetach) -> Result<Self, FcError> {
        match self.detach_drive_inner(&request) {
            Ok(()) => Ok(self),
            Err(err) => Err(discard_after_hotplug_failure(self, err)),
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

        let vsock_uds = self.jail.jail_root().join(VSOCK_SOCKET);
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

    fn detach_drive_inner(&mut self, request: &HotplugDriveDetach) -> Result<(), FcError> {
        self.prepare_hotplug_activity()?;
        validate_detach_request(request)?;
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
        let vsock_uds = self.jail.jail_root().join(VSOCK_SOCKET);
        let request_id = request_id_for(&self.vm_id, self.request_id.as_deref(), "drive_detach");
        let envelope = Envelope::with_request_id(
            DriveDetachRequest {
                devices: vec![DriveDetachSpec {
                    drive_id: drive_id.clone(),
                    mount_path: request.mount_path.clone(),
                }],
            },
            request_id,
        );
        let t_detach = Instant::now();
        let mut channel = send_envelope_with_open_retry(&vsock_uds, &self.vm_id, &envelope)?;
        let response: Envelope<DriveDetachResponse> = channel
            .recv()
            .map_err(|e| super::protocol::recv_error(e, "drive detach"))?;
        validate_detach_response(&response.payload, &drive_id)?;
        phase_event("hotplug_drive_detach", &self.vm_id, t_detach.elapsed());

        let patch = PartialDriveConfig {
            drive_id,
            path_on_host: Some(preallocated_drive_slot_jail_path(request.slot)),
        };
        let t_patch = Instant::now();
        self.client.patch_drive(&patch)?;
        phase_event(
            "hotplug_drive_placeholder_patch",
            &self.vm_id,
            t_patch.elapsed(),
        );
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

fn validate_detach_request(request: &HotplugDriveDetach) -> Result<(), FcError> {
    if request.mount_path.is_empty() {
        return Err(FcError::Config(ConfigError::InvalidValue {
            field: "mount_path",
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

fn validate_detach_response(response: &DriveDetachResponse, drive_id: &str) -> Result<(), FcError> {
    let status = response
        .statuses
        .iter()
        .find(|status| status.drive_id == drive_id)
        .ok_or_else(|| {
            super::protocol::unexpected_frame(
                "drive detach",
                "status for requested drive",
                "missing",
            )
        })?;
    match status.status {
        DriveDetachStatusKind::Detached | DriveDetachStatusKind::NotMounted => Ok(()),
        DriveDetachStatusKind::Failed => Err(FcError::DriveHotplug(
            status.error.unwrap_or(DriveHotplugError::UnmountFailed),
        )),
    }
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

fn discard_after_hotplug_failure(sandbox: RunningSandbox, err: FcError) -> FcError {
    if let Err(cleanup) = sandbox.force_kill().and_then(|stopped| stopped.delete()) {
        tracing::warn!(
            error = %cleanup,
            "failed to discard sandbox after drive hotplug failure"
        );
    }
    err
}

#[cfg(test)]
mod tests;
