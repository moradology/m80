//! Per-VM rootfs: shared base + sparse overlay allocation.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::StorageError;

/// A per-VM rootfs view: shared read-only base ext4 + per-VM writable overlay.
///
/// Constructed via [`Rootfs::prepare`] (allocates and formats the overlay)
/// or [`Rootfs::new_at`] (wraps an existing pair without I/O).
#[derive(Debug)]
pub struct Rootfs {
    base: PathBuf,
    overlay: PathBuf,
}

impl Rootfs {
    /// Produce the per-VM overlay ext4 and return a `Rootfs` pointing at the
    /// shared base and the new overlay.
    ///
    /// Allocates a sparse file at `overlay_dest` using
    /// `File::set_len(overlay_size_bytes)`, then formats it with
    /// `mkfs.ext4 -F <overlay_dest>`.  The base is NOT copied.
    ///
    /// Caller is responsible for sha256 verification of `base` via
    /// `m80_image_manifest::Manifest::verify()` before calling `prepare`.
    /// This function does not re-verify.
    ///
    /// `overlay_dest`'s parent directory must already exist.  `prepare` does
    /// NOT create parent directories — a missing parent returns
    /// [`StorageError::OverlayCreateFailed`].  (CLAUDE.md: "no silent recovery")
    pub fn prepare(
        base: &Path,
        overlay_dest: &Path,
        overlay_size_bytes: u64,
    ) -> Result<Self, StorageError> {
        // Allocate the sparse file.  Parent must exist; if it doesn't,
        // File::create surfaces ENOENT which we wrap as OverlayCreateFailed.
        let file = File::create(overlay_dest).map_err(|e| StorageError::OverlayCreateFailed {
            path: overlay_dest.to_path_buf(),
            err: e,
        })?;
        file.set_len(overlay_size_bytes)
            .map_err(|e| StorageError::OverlayCreateFailed {
                path: overlay_dest.to_path_buf(),
                err: e,
            })?;
        drop(file);

        // Format the sparse file as ext4.
        let out = Command::new("mkfs.ext4")
            .arg("-F")
            .arg(overlay_dest)
            .output()
            .map_err(|e| StorageError::OverlayCreateFailed {
                path: overlay_dest.to_path_buf(),
                err: e,
            })?;

        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr).trim().to_owned();
            let stdout = String::from_utf8_lossy(&out.stdout).trim().to_owned();
            return Err(StorageError::MkfsFailed {
                path: overlay_dest.to_path_buf(),
                status: out.status.code().unwrap_or(-1),
                stderr: if stderr.is_empty() { stdout } else { stderr },
            });
        }

        Ok(Self {
            base: base.to_path_buf(),
            overlay: overlay_dest.to_path_buf(),
        })
    }

    /// Wrap an existing `(base, overlay)` pair without performing any I/O.
    ///
    /// Intended for tests and recovery scenarios where both files are already
    /// in place.
    pub fn new_at(base: &Path, overlay: &Path) -> Self {
        Self {
            base: base.to_path_buf(),
            overlay: overlay.to_path_buf(),
        }
    }

    /// The shared, read-only base ext4.  Same host file across all VMs from
    /// this image; host page cache deduplicates.
    pub fn base_path(&self) -> &Path {
        &self.base
    }

    /// The per-VM writable overlay ext4 produced by [`Rootfs::prepare`].
    pub fn overlay_path(&self) -> &Path {
        &self.overlay
    }
}
