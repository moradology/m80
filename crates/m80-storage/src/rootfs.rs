//! Per-VM rootfs cloning.

use std::path::{Path, PathBuf};

use crate::{StorageError, io_err};

/// A per-VM rootfs ext4 clone.
///
/// Wraps the path of a single-use copy of an immutable base ext4 image.
/// Each VM gets its own clone; writes inside the VM never touch the base.
#[derive(Debug)]
pub struct Rootfs {
    path: PathBuf,
}

impl Rootfs {
    /// Clone the immutable base ext4 at `base` into a per-VM clone at `dest`.
    ///
    /// Caller is responsible for sha256 verification via
    /// `m80_image_manifest::Manifest::verify()` before calling `clone`.
    /// This function is a byte-for-byte file copy only; it does not re-verify.
    ///
    /// Parent directories of `dest` must already exist; this function does not
    /// create them. (Use `std::fs::create_dir_all` on `dest.parent()` first.)
    pub fn clone(base: &Path, dest: &Path) -> Result<Self, StorageError> {
        std::fs::copy(base, dest).map_err(|e| io_err(dest, e))?;
        Ok(Self {
            path: dest.to_path_buf(),
        })
    }

    /// Wrap an existing path as a `Rootfs` without performing a clone.
    ///
    /// Intended for tests and recovery scenarios where the image is already in
    /// place.
    pub fn new_at(dest: &Path) -> Self {
        Self {
            path: dest.to_path_buf(),
        }
    }

    /// Path of this rootfs clone.
    pub fn path(&self) -> &Path {
        &self.path
    }
}
