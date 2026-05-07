//! Boot artifact identity record written after preboot wiring succeeds.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use m80_image_manifest::{ImageKind, KernelKind, Manifest};
use m80_preflight::Discovery;

use crate::error::{ConfigError, FcError};

/// Current `boot-identity.json` schema version.
const BOOT_IDENTITY_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BootIdentity {
    schema_version: u32,
    kernel: PathBuf,
    kernel_kind: KernelKind,
    kernel_sha256: String,
    rootfs: PathBuf,
    rootfs_sha256: String,
    manifest_sha256: String,
    image_kind: ImageKind,
    expected_firecracker_version: String,
    guest_port: u32,
    ready_marker: String,
    boot_target: Option<String>,
}

/// Write `<run_dir>/boot-identity.json` from already-validated discovery data.
pub(crate) fn record(run_dir: &Path, discovery: &Discovery) -> Result<(), FcError> {
    let identity = BootIdentity::from_discovery(discovery)?;
    let path = crate::layout::boot_identity_path(run_dir);
    let bytes = serde_json::to_vec_pretty(&identity)
        .map_err(|e| FcError::Config(ConfigError::Other(format!("serialize boot identity: {e}"))))?;
    std::fs::write(path, bytes).map_err(FcError::Io)
}

impl BootIdentity {
    fn from_discovery(discovery: &Discovery) -> Result<Self, FcError> {
        let manifest = &discovery.manifest;
        Ok(Self {
            schema_version: BOOT_IDENTITY_SCHEMA_VERSION,
            kernel: discovery.kernel.clone(),
            kernel_kind: manifest.kernel_kind,
            kernel_sha256: manifest.kernel_image_sha256.clone(),
            rootfs: discovery.rootfs.clone(),
            rootfs_sha256: manifest.output_rootfs_sha256.clone(),
            manifest_sha256: manifest_sha256(manifest)?,
            image_kind: manifest.image_kind,
            expected_firecracker_version: manifest.expected_firecracker_version.clone(),
            guest_port: manifest.guest_port,
            ready_marker: manifest.ready_marker.clone(),
            boot_target: manifest.boot_target.clone(),
        })
    }
}

fn manifest_sha256(manifest: &Manifest) -> Result<String, FcError> {
    let bytes = serde_json::to_vec(manifest).map_err(|e| {
        FcError::Config(ConfigError::Other(format!(
            "serialize manifest for boot identity: {e}"
        )))
    })?;
    Ok(hex::encode(Sha256::digest(bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn discovery() -> Discovery {
        Discovery {
            firecracker_bin: PathBuf::from("/bin/firecracker"),
            jailer_bin: PathBuf::from("/bin/jailer"),
            kernel: PathBuf::from("/artifacts/vmlinux"),
            rootfs: PathBuf::from("/artifacts/output.ext4"),
            manifest: Manifest {
                schema_version: m80_image_manifest::SCHEMA_VERSION,
                image_kind: ImageKind::Minimal,
                kernel_kind: KernelKind::Stripped,
                expected_firecracker_version: "v1.10.0".into(),
                kernel_image: PathBuf::from("vmlinux"),
                kernel_image_sha256: "a".repeat(64),
                output_rootfs_image: PathBuf::from("output.ext4"),
                output_rootfs_sha256: "b".repeat(64),
                source_rootfs_image: None,
                source_rootfs_sha256: None,
                daemon_binary_path: PathBuf::from("m80-guestd"),
                daemon_binary_sha256: "c".repeat(64),
                service_unit_path: None,
                service_unit_sha256: None,
                workspace_mount_path: None,
                workspace_mount_sha256: None,
                boot_target: Some("basic.target".into()),
                guest_port: 9001,
                no_egress_reason: None,
                ready_marker: "GUESTD_READY".into(),
            },
            run_root: PathBuf::from("/run/m80"),
            privilege: m80_preflight::PrivilegeStatus::Root,
            report: vec![],
        }
    }

    #[test]
    fn boot_identity_from_discovery_carries_manifest_fields() {
        let discovery = discovery();

        let identity = BootIdentity::from_discovery(&discovery).unwrap();

        assert_eq!(identity.schema_version, BOOT_IDENTITY_SCHEMA_VERSION);
        assert_eq!(identity.kernel, PathBuf::from("/artifacts/vmlinux"));
        assert_eq!(identity.kernel_kind, KernelKind::Stripped);
        assert_eq!(identity.kernel_sha256, "a".repeat(64));
        assert_eq!(identity.rootfs, PathBuf::from("/artifacts/output.ext4"));
        assert_eq!(identity.rootfs_sha256, "b".repeat(64));
        assert_eq!(identity.image_kind, ImageKind::Minimal);
        assert_eq!(identity.expected_firecracker_version, "v1.10.0");
        assert_eq!(identity.guest_port, 9001);
        assert_eq!(identity.ready_marker, "GUESTD_READY");
        assert_eq!(identity.boot_target.as_deref(), Some("basic.target"));
        assert_eq!(identity.manifest_sha256.len(), 64);
    }

    #[test]
    fn record_writes_boot_identity_json_under_run_dir() {
        let dir = tempfile::tempdir().unwrap();
        let discovery = discovery();

        record(dir.path(), &discovery).unwrap();

        let path = crate::layout::boot_identity_path(dir.path());
        let text = std::fs::read_to_string(path).unwrap();
        let identity: BootIdentity = serde_json::from_str(&text).unwrap();
        assert_eq!(identity.kernel, PathBuf::from("/artifacts/vmlinux"));
        assert_eq!(identity.rootfs, PathBuf::from("/artifacts/output.ext4"));
        assert_eq!(identity.ready_marker, "GUESTD_READY");
    }
}
