//! Schema and verification for the m80 guest-image provenance manifest.
//!
//! See `README.md` for the black-box contract.
//! Behavior captures: beads `m80-sz1.3`, `m80-sz1.4` (`br show m80-sz1.3`).

#![deny(missing_docs)]

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Manifest schema version. m80 v0.1 ships `1`; future versions are new code,
/// not migrations.
pub const SCHEMA_VERSION: u32 = 1;

/// The provenance manifest for a built guest image.
///
/// One file, written beside the rootfs as `<rootfs-path>.manifest.json`.
/// Both `m80-image-build` (writer) and `m80-preflight` (reader/verifier)
/// depend on this struct so the schema cannot drift.
///
/// Six artifacts are recorded with separate path + sha256 fields each:
/// kernel image, source rootfs, output rootfs, daemon binary, service unit
/// file, workspace-mount unit file. [`Manifest::verify`] covers the full set
/// from the manifest alone.
///
/// Field declaration order is alphabetical so the JSON serialization is
/// stable without a canonicalization pass.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// systemd boot target (typically `multi-user.target`).
    pub boot_target: String,
    /// Absolute path of the in-VM daemon binary embedded in the image.
    pub daemon_binary_path: PathBuf,
    /// sha256 hex digest of the daemon binary.
    pub daemon_binary_sha256: String,
    /// Firecracker version this image is pinned to (e.g., `"v1.15.1"`).
    pub expected_firecracker_version: String,
    /// Vsock port the in-VM daemon listens on.
    pub guest_port: u32,
    /// Absolute path of the kernel image at build time.
    pub kernel_image: PathBuf,
    /// sha256 hex digest of the kernel image bytes.
    pub kernel_image_sha256: String,
    /// Free-text reason recorded when the image is built without egress
    /// configured.
    pub no_egress_reason: Option<String>,
    /// Absolute path of the built rootfs (the one Firecracker mounts).
    pub output_rootfs_image: PathBuf,
    /// sha256 hex digest of the built rootfs bytes.
    pub output_rootfs_sha256: String,
    /// Serial-console marker the guest emits when the daemon is ready.
    pub ready_marker: String,
    /// Always [`SCHEMA_VERSION`] for v0.1.
    pub schema_version: u32,
    /// Absolute path of the systemd service unit file.
    pub service_unit_path: PathBuf,
    /// sha256 hex digest of the service unit file bytes.
    pub service_unit_sha256: String,
    /// Absolute path of the source rootfs (squashfs or upstream ext4).
    pub source_rootfs_image: PathBuf,
    /// sha256 hex digest of the source rootfs bytes.
    pub source_rootfs_sha256: String,
    /// Absolute path of the systemd workspace-mount unit file.
    pub workspace_mount_path: PathBuf,
    /// sha256 hex digest of the workspace-mount unit file bytes.
    pub workspace_mount_sha256: String,
}

/// Partial-deserialize struct used to extract `schema_version` BEFORE
/// committing to a full `Manifest` parse. Without this, a v0.2 manifest
/// stamped `schema_version: 2` plus a new field surfaces as a
/// `Json("unknown field …")` (because `Manifest` carries
/// `#[serde(deny_unknown_fields)]`) instead of `UnsupportedSchemaVersion(2)`.
#[derive(Deserialize)]
struct SchemaVersionProbe {
    schema_version: u32,
}

impl Manifest {
    /// Read and validate a manifest at `path`.
    ///
    /// Order of checks:
    /// 1. Read bytes.
    /// 2. Probe `schema_version` only; mismatch → [`ManifestError::UnsupportedSchemaVersion`].
    ///    Fires before structural-shape errors so a v0.2 manifest produces a
    ///    clear error instead of an unknown-field error.
    /// 3. Full parse into `Manifest`.
    ///
    /// sha256s are NOT verified here — call [`Manifest::verify`] for that.
    pub fn read(path: &Path) -> Result<Manifest, ManifestError> {
        let raw = std::fs::read(path).map_err(|source| ManifestError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let probe: SchemaVersionProbe = serde_json::from_slice(&raw)?;
        if probe.schema_version != SCHEMA_VERSION {
            return Err(ManifestError::UnsupportedSchemaVersion(
                probe.schema_version,
            ));
        }
        let manifest: Manifest = serde_json::from_slice(&raw)?;
        Ok(manifest)
    }

    /// Write this manifest to `path` as pretty-printed JSON with a trailing
    /// newline and mode 0644 on Unix.
    ///
    /// Field order in the output is alphabetical (struct field order). The
    /// caller is responsible for the parent directory existing.
    pub fn write(&self, path: &Path) -> Result<(), ManifestError> {
        let mut json = serde_json::to_string_pretty(self)?;
        json.push('\n');
        std::fs::write(path, json.as_bytes()).map_err(|source| ManifestError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o644);
            std::fs::set_permissions(path, perms).map_err(|source| ManifestError::Io {
                path: path.to_path_buf(),
                source,
            })?;
        }
        Ok(())
    }

    /// Recompute every recorded sha256 from the on-disk artifact and compare
    /// against the manifest. Any mismatch is fatal; there is no "warn and
    /// continue."
    ///
    /// Paths recorded in the manifest are used as-is when absolute, or
    /// resolved relative to `root` when relative. All six artifacts (kernel,
    /// source rootfs, output rootfs, daemon binary, service unit, workspace
    /// mount unit) are covered.
    pub fn verify(&self, root: &Path) -> Result<(), ManifestError> {
        let resolve = |p: &Path| -> PathBuf {
            if p.is_absolute() {
                p.to_path_buf()
            } else {
                root.join(p)
            }
        };

        check_sha256(
            "kernel_image",
            &resolve(&self.kernel_image),
            &self.kernel_image_sha256,
        )?;
        check_sha256(
            "source_rootfs_image",
            &resolve(&self.source_rootfs_image),
            &self.source_rootfs_sha256,
        )?;
        check_sha256(
            "output_rootfs_image",
            &resolve(&self.output_rootfs_image),
            &self.output_rootfs_sha256,
        )?;
        check_sha256(
            "daemon_binary_path",
            &resolve(&self.daemon_binary_path),
            &self.daemon_binary_sha256,
        )?;
        check_sha256(
            "service_unit_path",
            &resolve(&self.service_unit_path),
            &self.service_unit_sha256,
        )?;
        check_sha256(
            "workspace_mount_path",
            &resolve(&self.workspace_mount_path),
            &self.workspace_mount_sha256,
        )?;
        Ok(())
    }
}

fn check_sha256(field: &str, path: &Path, expected: &str) -> Result<(), ManifestError> {
    let bytes = std::fs::read(path).map_err(|source| ManifestError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let actual = hex::encode(Sha256::digest(&bytes));
    if expected != actual {
        return Err(ManifestError::Sha256Mismatch {
            field: field.to_owned(),
            expected: expected.to_owned(),
            actual,
        });
    }
    Ok(())
}

/// Errors surfaced by manifest read / write / verify.
#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    /// `schema_version` was not [`SCHEMA_VERSION`]. Reported via the
    /// `SchemaVersionProbe` partial parse before `deny_unknown_fields`
    /// errors fire.
    #[error("unsupported manifest schema version: got {0}, expected {SCHEMA_VERSION}")]
    UnsupportedSchemaVersion(u32),
    /// A recomputed sha256 did not match the recorded value.
    #[error("sha256 mismatch on {field}: expected {expected}, got {actual}")]
    Sha256Mismatch {
        /// Manifest field whose hash failed (e.g., `"kernel_image"`).
        field: String,
        /// Recorded hex digest.
        expected: String,
        /// Recomputed hex digest.
        actual: String,
    },
    /// I/O failure on a manifest read or artifact read; carries the path so
    /// the caller doesn't have to guess which file failed.
    #[error("i/o on {}: {source}", path.display())]
    Io {
        /// File the I/O was attempted against.
        path: PathBuf,
        /// Underlying I/O error.
        #[source]
        source: io::Error,
    },
    /// JSON encode/decode failure (malformed JSON, missing required field,
    /// or unknown field rejected by `deny_unknown_fields`).
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_tempdir() -> tempfile::TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    fn sample_manifest(root: &Path) -> Manifest {
        for name in &[
            "vmlinux",
            "source.ext4",
            "output.ext4",
            "guestd",
            "guestd.service",
            "workspace.mount",
        ] {
            std::fs::write(root.join(name), name.as_bytes()).unwrap();
        }
        Manifest {
            boot_target: "multi-user.target".into(),
            daemon_binary_path: root.join("guestd"),
            daemon_binary_sha256: hex::encode(Sha256::digest(b"guestd")),
            expected_firecracker_version: "v1.15.1".into(),
            guest_port: 9001, // matches m80_proto::GUEST_PORT_DEFAULT
            kernel_image: root.join("vmlinux"),
            kernel_image_sha256: hex::encode(Sha256::digest(b"vmlinux")),
            no_egress_reason: None,
            output_rootfs_image: root.join("output.ext4"),
            output_rootfs_sha256: hex::encode(Sha256::digest(b"output.ext4")),
            ready_marker: "GUESTD_READY".into(), // matches m80_proto::READY_MARKER_DEFAULT
            schema_version: SCHEMA_VERSION,
            service_unit_path: root.join("guestd.service"),
            service_unit_sha256: hex::encode(Sha256::digest(b"guestd.service")),
            source_rootfs_image: root.join("source.ext4"),
            source_rootfs_sha256: hex::encode(Sha256::digest(b"source.ext4")),
            workspace_mount_path: root.join("workspace.mount"),
            workspace_mount_sha256: hex::encode(Sha256::digest(b"workspace.mount")),
        }
    }

    #[test]
    fn round_trip_byte_equal() {
        let dir = make_tempdir();
        let m = sample_manifest(dir.path());
        let path = dir.path().join("rootfs.ext4.manifest.json");
        m.write(&path).unwrap();
        let raw1 = std::fs::read(&path).unwrap();
        let m2 = Manifest::read(&path).unwrap();
        let path2 = dir.path().join("rootfs2.ext4.manifest.json");
        m2.write(&path2).unwrap();
        let raw2 = std::fs::read(&path2).unwrap();
        assert_eq!(raw1, raw2, "round-trip must be byte-identical");
    }

    #[test]
    fn trailing_newline_present() {
        let dir = make_tempdir();
        let m = sample_manifest(dir.path());
        let path = dir.path().join("m.json");
        m.write(&path).unwrap();
        let raw = std::fs::read(&path).unwrap();
        assert_eq!(raw.last(), Some(&b'\n'), "output must end with newline");
    }

    fn write_mutated_and_read(
        dir: &Path,
        mutate: impl FnOnce(&mut serde_json::Value),
    ) -> Result<Manifest, ManifestError> {
        let m = sample_manifest(dir);
        let mut v = serde_json::to_value(&m).unwrap();
        mutate(&mut v);
        let raw = format!("{}\n", serde_json::to_string_pretty(&v).unwrap());
        let path = dir.join("mutated.json");
        std::fs::write(&path, raw.as_bytes()).unwrap();
        Manifest::read(&path)
    }

    #[test]
    fn deny_unknown_fields_rejects_extra_key() {
        let dir = make_tempdir();
        let err = write_mutated_and_read(dir.path(), |v| {
            v.as_object_mut()
                .unwrap()
                .insert("EXTRA_FIELD".into(), serde_json::json!("forbidden"));
        })
        .unwrap_err();
        assert!(
            matches!(err, ManifestError::Json(_)),
            "unknown field must surface as Json error, got {err:?}"
        );
    }

    #[test]
    fn schema_version_mismatch_returns_correct_error() {
        let dir = make_tempdir();
        let err = write_mutated_and_read(dir.path(), |v| {
            v["schema_version"] = serde_json::json!(99u32);
        })
        .unwrap_err();
        assert!(
            matches!(err, ManifestError::UnsupportedSchemaVersion(99)),
            "expected UnsupportedSchemaVersion(99), got {err:?}"
        );
    }

    /// Regression: a future-version manifest with extra fields surfaces as
    /// `UnsupportedSchemaVersion`, not `Json("unknown field …")`. The probe
    /// fires before `deny_unknown_fields`.
    #[test]
    fn schema_version_check_fires_before_unknown_field_check() {
        let dir = make_tempdir();
        let err = write_mutated_and_read(dir.path(), |v| {
            v["schema_version"] = serde_json::json!(2u32);
            v.as_object_mut()
                .unwrap()
                .insert("future_field".into(), serde_json::json!("v0.2 stuff"));
        })
        .unwrap_err();
        assert!(
            matches!(err, ManifestError::UnsupportedSchemaVersion(2)),
            "expected UnsupportedSchemaVersion(2) (probe fires first), got {err:?}"
        );
    }

    #[test]
    fn verify_passes_for_correct_artifacts() {
        let dir = make_tempdir();
        let m = sample_manifest(dir.path());
        m.verify(dir.path()).unwrap();
    }

    #[test]
    fn verify_detects_tampered_kernel() {
        let dir = make_tempdir();
        let mut m = sample_manifest(dir.path());
        m.kernel_image_sha256 = "deadbeef".repeat(8);
        let err = m.verify(dir.path()).unwrap_err();
        assert!(
            matches!(
                &err,
                ManifestError::Sha256Mismatch { field, .. } if field == "kernel_image"
            ),
            "expected Sha256Mismatch on kernel_image, got {err:?}"
        );
    }

    #[test]
    fn verify_detects_tampered_service_unit() {
        let dir = make_tempdir();
        let mut m = sample_manifest(dir.path());
        m.service_unit_sha256 = "deadbeef".repeat(8);
        let err = m.verify(dir.path()).unwrap_err();
        assert!(
            matches!(
                &err,
                ManifestError::Sha256Mismatch { field, .. } if field == "service_unit_path"
            ),
            "expected Sha256Mismatch on service_unit_path, got {err:?}"
        );
    }

    #[test]
    fn verify_detects_tampered_workspace_mount_unit() {
        let dir = make_tempdir();
        let mut m = sample_manifest(dir.path());
        m.workspace_mount_sha256 = "deadbeef".repeat(8);
        let err = m.verify(dir.path()).unwrap_err();
        assert!(
            matches!(
                &err,
                ManifestError::Sha256Mismatch { field, .. } if field == "workspace_mount_path"
            ),
            "expected Sha256Mismatch on workspace_mount_path, got {err:?}"
        );
    }

    #[test]
    fn verify_missing_artifact_surfaces_io_error_with_path() {
        let dir = make_tempdir();
        let mut m = sample_manifest(dir.path());
        let missing = dir.path().join("does_not_exist");
        m.kernel_image = missing.clone();
        let err = m.verify(dir.path()).unwrap_err();
        match err {
            ManifestError::Io { path, source } => {
                assert_eq!(path, missing, "Io variant must carry the failed path");
                assert_eq!(source.kind(), io::ErrorKind::NotFound);
            }
            other => panic!("expected Io error with path, got {other:?}"),
        }
    }

    #[test]
    fn file_mode_is_0644() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let dir = make_tempdir();
            let m = sample_manifest(dir.path());
            let path = dir.path().join("mode_test.json");
            m.write(&path).unwrap();
            let meta = std::fs::metadata(&path).unwrap();
            let mode = meta.mode() & 0o777;
            assert_eq!(mode, 0o644, "manifest file mode must be 0644, got {mode:o}");
        }
    }

    #[test]
    fn keys_are_in_alphabetical_order() {
        let dir = make_tempdir();
        let m = sample_manifest(dir.path());
        let path = dir.path().join("key_order.json");
        m.write(&path).unwrap();
        let raw = std::fs::read_to_string(&path).unwrap();
        let keys: Vec<&str> = raw
            .lines()
            .filter_map(|l| {
                let trimmed = l.trim();
                if trimmed.starts_with('"') {
                    trimmed.split('"').nth(1)
                } else {
                    None
                }
            })
            .collect();
        let mut sorted = keys.clone();
        sorted.sort_unstable();
        assert_eq!(keys, sorted, "JSON keys must be in alphabetical order");
    }
}
