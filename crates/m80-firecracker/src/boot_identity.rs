//! Boot artifact identity record written after preboot wiring succeeds.

use std::io::Write as _;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use m80_image_manifest::{ImageKind, KernelKind, Manifest, RootfsFormat};
use m80_preflight::Discovery;

use crate::error::FcError;

/// Current `boot-identity.json` schema version.
const BOOT_IDENTITY_SCHEMA_VERSION: u32 = 1;
const BOOT_IDENTITY_FILE_MODE: u32 = 0o600;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BootIdentity {
    schema_version: u32,
    kernel: PathBuf,
    kernel_kind: KernelKind,
    kernel_sha256: String,
    rootfs: PathBuf,
    rootfs_format: RootfsFormat,
    rootfs_sha256: String,
    manifest_sha256: String,
    image_kind: ImageKind,
    expected_firecracker_version: String,
    guest_port: u32,
    ready_marker: String,
}

/// Write `<run_dir>/boot-identity.json` from already-validated discovery data.
pub(crate) fn record(run_dir: &Path, discovery: &Discovery) -> Result<(), FcError> {
    let identity = BootIdentity::from_discovery(discovery)?;
    let path = crate::layout::boot_identity_path(run_dir);
    let bytes = serde_json::to_vec_pretty(&identity).map_err(|source| FcError::Json {
        context: "serialize boot identity",
        source,
    })?;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(BOOT_IDENTITY_FILE_MODE)
        .open(&path)
        .map_err(|source| FcError::PathIo {
            path: path.clone(),
            source,
        })?;
    file.set_permissions(std::fs::Permissions::from_mode(BOOT_IDENTITY_FILE_MODE))
        .map_err(|source| FcError::PathIo {
            path: path.clone(),
            source,
        })?;
    file.write_all(&bytes).map_err(|source| FcError::PathIo {
        path: path.clone(),
        source,
    })?;
    Ok(())
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
            rootfs_format: manifest.rootfs_format,
            rootfs_sha256: manifest.output_rootfs_sha256.clone(),
            manifest_sha256: manifest_sha256(manifest)?,
            image_kind: manifest.image_kind,
            expected_firecracker_version: manifest.expected_firecracker_version.clone(),
            guest_port: manifest.guest_port,
            ready_marker: manifest.ready_marker.clone(),
        })
    }
}

fn manifest_sha256(manifest: &Manifest) -> Result<String, FcError> {
    let bytes = serde_json::to_vec(manifest).map_err(|source| FcError::Json {
        context: "serialize manifest for boot identity",
        source,
    })?;
    Ok(hex::encode(Sha256::digest(bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn discovery() -> Discovery {
        let rootfs = tempfile::NamedTempFile::new().unwrap();
        let rootfs_path = rootfs.path().to_path_buf();
        let rootfs_file = rootfs.reopen().unwrap();
        Discovery {
            firecracker_bin: PathBuf::from("/bin/firecracker"),
            firecracker_seccomp_filter: PathBuf::from("/bin/firecracker-seccomp-filter.bin"),
            jailer_bin: PathBuf::from("/bin/jailer"),
            jailer_harden_bin: PathBuf::from("/bin/m80-jailer-harden"),
            kernel: PathBuf::from("/artifacts/vmlinux"),
            rootfs: PathBuf::from("/artifacts/output.ext4"),
            pinned_rootfs: m80_preflight::PinnedRootfs::from_file(rootfs_path, rootfs_file),
            manifest: Manifest::new(
                PathBuf::from("m80-guestd"),
                "c".repeat(64),
                "v1.10.0".into(),
                9001,
                ImageKind::Minimal,
                PathBuf::from("vmlinux"),
                "a".repeat(64),
                KernelKind::Stripped,
                None,
                PathBuf::from("output.ext4"),
                "b".repeat(64),
                "GUESTD_READY".into(),
                RootfsFormat::Ext4,
                None,
                None,
            ),
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
        assert_eq!(identity.rootfs_format, RootfsFormat::Ext4);
        assert_eq!(identity.rootfs_sha256, "b".repeat(64));
        assert_eq!(identity.image_kind, ImageKind::Minimal);
        assert_eq!(identity.expected_firecracker_version, "v1.10.0");
        assert_eq!(identity.guest_port, 9001);
        assert_eq!(identity.ready_marker, "GUESTD_READY");
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

    #[test]
    fn record_writes_owner_only_boot_identity() {
        let dir = tempfile::tempdir().unwrap();
        let discovery = discovery();

        record(dir.path(), &discovery).unwrap();

        let path = crate::layout::boot_identity_path(dir.path());
        let mode = std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, BOOT_IDENTITY_FILE_MODE);
    }

    #[test]
    fn record_corrects_existing_permissive_boot_identity() {
        let dir = tempfile::tempdir().unwrap();
        let path = crate::layout::boot_identity_path(dir.path());
        std::fs::write(&path, b"old").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        let discovery = discovery();

        record(dir.path(), &discovery).unwrap();

        let mode = std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, BOOT_IDENTITY_FILE_MODE);
    }
}
