//! Host-side pmem layer guest mount phase.

use std::path::Path;
use std::time::Instant;

use m80_proto::{
    Envelope, PmemMountError, PmemMountRequest, PmemMountResponse, PmemMountSpec,
    PmemMountStatusKind,
};

use crate::diagnostics::phase_event;
use crate::error::FcError;
use crate::lifecycle::exec::{request_id_for, send_envelope_with_open_retry};
use crate::pmem::PmemLayer;

/// Ask guestd to mount all declared pmem layers after it has reported ready.
pub(crate) fn phase_13_pmem_guest_mount(
    vsock_uds: &Path,
    vm_id: &str,
    base_request_id: Option<&str>,
    firecracker_pid: u32,
    layers: &[PmemLayer],
) -> Result<(), FcError> {
    if layers.is_empty() {
        return Ok(());
    }

    let expected = build_pmem_mount_specs(layers);
    let request_id = request_id_for(vm_id, base_request_id, "pmem_mount");
    let envelope = Envelope::with_request_id(
        PmemMountRequest {
            devices: expected.clone(),
        },
        request_id.clone(),
    );
    let mut channel =
        send_envelope_with_open_retry(vsock_uds, vm_id, firecracker_pid, "pmem mount", &envelope)?;
    let t = Instant::now();
    let response: Envelope<PmemMountResponse> = channel
        .recv()
        .map_err(|e| super::protocol::recv_error(e, "pmem mount", firecracker_pid))?;
    validate_pmem_mount_response(&response, &request_id, &expected)?;
    phase_event("pmem_guest_mount", vm_id, t.elapsed());
    Ok(())
}

fn build_pmem_mount_specs(layers: &[PmemLayer]) -> Vec<PmemMountSpec> {
    layers
        .iter()
        .enumerate()
        .map(|(slot, layer)| PmemMountSpec {
            device_path: format!("/dev/pmem{slot}"),
            mount_path: layer.mount_at().as_path().to_string_lossy().into_owned(),
            digest_hex: layer.image().digest().as_str().to_owned(),
        })
        .collect()
}

fn validate_pmem_mount_response(
    response: &Envelope<PmemMountResponse>,
    request_id: &str,
    expected: &[PmemMountSpec],
) -> Result<(), FcError> {
    if response.request_id.as_deref() != Some(request_id) {
        return Err(super::protocol::request_id_mismatch(
            "pmem mount",
            request_id,
            response.request_id.clone(),
        ));
    }

    for status in &response.payload.statuses {
        if status.status == PmemMountStatusKind::Failed {
            return Err(FcError::PmemMount(
                status.error.unwrap_or(PmemMountError::Io),
            ));
        }
    }

    for spec in expected {
        let status = response
            .payload
            .statuses
            .iter()
            .find(|status| {
                status.device_path == spec.device_path && status.mount_path == spec.mount_path
            })
            .ok_or_else(|| {
                super::protocol::unexpected_frame(
                    "pmem mount",
                    "status for requested pmem device",
                    "missing",
                )
            })?;
        match status.status {
            PmemMountStatusKind::Mounted | PmemMountStatusKind::AlreadyMounted => {}
            PmemMountStatusKind::Failed => unreachable!("failed statuses returned above"),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use m80_proto::{PmemMountStatus, PmemMountStatusKind};

    use super::*;
    use crate::error::WireProtocolError;
    use crate::pmem::{ErofsImageRef, GuestMountPath, ImageDigest, PmemLayer, PmemSharing};

    fn layer(digest: &str, mount_at: &str) -> PmemLayer {
        PmemLayer::new(
            ErofsImageRef::from_digest(ImageDigest::parse(digest).unwrap()),
            PmemSharing::PerVm,
            GuestMountPath::parse(mount_at).unwrap(),
        )
    }

    fn mounted_status(device_path: &str, mount_path: &str) -> PmemMountStatus {
        PmemMountStatus {
            device_path: device_path.to_owned(),
            mount_path: mount_path.to_owned(),
            status: PmemMountStatusKind::Mounted,
            error: None,
        }
    }

    #[test]
    fn specs_use_slot_indexed_devices_and_layer_contract() {
        let layers = vec![
            layer(&"a".repeat(64), "/opt/m80-layers/rust"),
            layer(&"b".repeat(64), "/opt/m80-layers/cargo"),
        ];

        let specs = build_pmem_mount_specs(&layers);

        assert_eq!(specs[0].device_path, "/dev/pmem0");
        assert_eq!(specs[0].mount_path, "/opt/m80-layers/rust");
        assert_eq!(specs[0].digest_hex, "a".repeat(64));
        assert_eq!(specs[1].device_path, "/dev/pmem1");
        assert_eq!(specs[1].mount_path, "/opt/m80-layers/cargo");
        assert_eq!(specs[1].digest_hex, "b".repeat(64));
    }

    #[test]
    fn failed_dax_response_maps_to_typed_error() {
        let expected = vec![PmemMountSpec {
            device_path: "/dev/pmem0".to_owned(),
            mount_path: "/opt/m80-layers/rust".to_owned(),
            digest_hex: "a".repeat(64),
        }];
        let response = Envelope::with_request_id(
            PmemMountResponse {
                statuses: vec![PmemMountStatus {
                    device_path: "/dev/pmem0".to_owned(),
                    mount_path: "/opt/m80-layers/rust".to_owned(),
                    status: PmemMountStatusKind::Failed,
                    error: Some(PmemMountError::DaxFlagAbsent),
                }],
            },
            "req-pmem",
        );

        let err = validate_pmem_mount_response(&response, "req-pmem", &expected).unwrap_err();

        assert!(matches!(
            err,
            FcError::PmemMount(PmemMountError::DaxFlagAbsent)
        ));
    }

    #[test]
    fn missing_requested_status_is_protocol_error() {
        let expected = vec![PmemMountSpec {
            device_path: "/dev/pmem0".to_owned(),
            mount_path: "/opt/m80-layers/rust".to_owned(),
            digest_hex: "a".repeat(64),
        }];
        let response = Envelope::with_request_id(
            PmemMountResponse {
                statuses: vec![mounted_status("/dev/pmem1", "/opt/m80-layers/cargo")],
            },
            "req-pmem",
        );

        let err = validate_pmem_mount_response(&response, "req-pmem", &expected).unwrap_err();

        assert!(matches!(
            err,
            FcError::Protocol(WireProtocolError::UnexpectedFrame {
                context: "pmem mount",
                expected: "status for requested pmem device",
                ..
            })
        ));
    }
}
