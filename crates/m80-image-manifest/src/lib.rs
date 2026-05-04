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

/// The provenance manifest for a built guest image. Single source of truth
/// shared by `m80-image-build` (writer) and `m80-preflight` (reader/verifier)
/// so the schema cannot drift. Field declaration order is alphabetical so
/// JSON serialization is byte-stable without a canonicalization pass.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// systemd boot target (typically `multi-user.target`).
    pub boot_target: String,
    /// Host-side audit copy of the daemon binary that was installed into
    /// the image. `Manifest::verify` recomputes the sha256 of this file at
    /// preflight time without loop-mounting the rootfs. The in-VM
    /// destination is a build constant, not stored here.
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

/// Probes only `schema_version` so a v0.2 manifest reports
/// `UnsupportedSchemaVersion(2)` instead of leaking the unrelated
/// `Json("unknown field …")` from `deny_unknown_fields` on the v0.1 struct.
#[derive(Deserialize)]
struct SchemaVersionProbe {
    schema_version: u32,
}

impl Manifest {
    /// Read and structurally validate a manifest. Probes `schema_version`
    /// before the full parse so future-version files report cleanly.
    /// Does NOT verify sha256s — call [`Manifest::verify`] for that.
    pub fn read(path: &Path) -> Result<Manifest, ManifestError> {
        let raw = std::fs::read(path).map_err(|source| ManifestError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let probe: SchemaVersionProbe = serde_json::from_slice(&raw)?;
        if probe.schema_version != SCHEMA_VERSION {
            return Err(ManifestError::UnsupportedSchemaVersion(probe.schema_version));
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
    use std::io::Read;
    // Stream the file through Sha256 in 64 KiB chunks so verifying a
    // multi-GiB rootfs doesn't allocate the whole file on the heap.
    let mut file = std::fs::File::open(path).map_err(|source| ManifestError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf).map_err(|source| ManifestError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let actual = hex::encode(hasher.finalize());
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

