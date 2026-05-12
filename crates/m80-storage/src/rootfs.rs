//! Per-VM rootfs: shared base + cloned empty overlay allocation.

use std::fs::{File, OpenOptions};
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use crate::{format_exit, StorageError};

const TEMPLATE_SCHEMA_VERSION: u32 = 1;
const TEMPLATE_LOCK_TIMEOUT: Duration = Duration::from_secs(10);
const TEMPLATE_CLONE_ARGS: &[&str] = &["--reflink=auto", "--sparse=always"];

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
    /// Ensures a run-root-local empty ext4 overlay template exists, then
    /// clones that template to `overlay_dest`.  The base is NOT copied.
    ///
    /// Caller is responsible for sha256 verification of `base` via
    /// `m80_image_manifest::Manifest::verify()` before calling `prepare`.
    /// This function does not re-verify.
    ///
    /// `overlay_dest`'s parent directory must already exist.  `prepare` does
    /// NOT create parent directories — a missing parent returns
    /// [`StorageError::OverlayCreateFailed`].
    pub fn prepare(
        base: &Path,
        overlay_dest: &Path,
        overlay_size_bytes: u64,
    ) -> Result<Self, StorageError> {
        let template = default_template_path(overlay_dest, overlay_size_bytes)?;
        ensure_template(&template, overlay_size_bytes)?;
        clone_template(&template, overlay_dest)?;

        Ok(Self {
            base: base.to_path_buf(),
            overlay: overlay_dest.to_path_buf(),
        })
    }

    /// Wrap an existing `(base, overlay)` pair without performing any I/O.
    ///
    /// Intended for tests and recovery scenarios where both files are already
    /// in place.
    #[must_use] pub fn new_at(base: &Path, overlay: &Path) -> Self {
        Self {
            base: base.to_path_buf(),
            overlay: overlay.to_path_buf(),
        }
    }

    /// The shared, read-only base ext4.  Same host file across all VMs from
    /// this image; host page cache deduplicates.
    #[must_use] pub fn base_path(&self) -> &Path {
        &self.base
    }

    /// The per-VM writable overlay ext4 produced by [`Rootfs::prepare`].
    #[must_use] pub fn overlay_path(&self) -> &Path {
        &self.overlay
    }
}

fn default_template_path(
    overlay_dest: &Path,
    overlay_size_bytes: u64,
) -> Result<PathBuf, StorageError> {
    let vm_dir = overlay_dest
        .parent()
        .ok_or_else(|| StorageError::OverlayCreateFailed {
            path: overlay_dest.to_path_buf(),
            err: std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "overlay path must have a parent directory",
            ),
        })?;
    let run_root = vm_dir
        .parent()
        .ok_or_else(|| StorageError::OverlayCreateFailed {
            path: overlay_dest.to_path_buf(),
            err: std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "overlay path must live under a run root",
            ),
        })?;
    Ok(run_root.join(format!(
        ".rootfs-overlay-template-v{TEMPLATE_SCHEMA_VERSION}-{overlay_size_bytes}.ext4"
    )))
}

fn ensure_template(template: &Path, size_bytes: u64) -> Result<(), StorageError> {
    let _lock = TemplateLock::acquire(&template.with_extension("lock"))?;
    if template.exists() {
        validate_template(template, size_bytes)?;
        return Ok(());
    }
    create_template(template, size_bytes)?;
    validate_template(template, size_bytes)
}

fn create_template(template: &Path, size_bytes: u64) -> Result<(), StorageError> {
    let parent = template
        .parent()
        .ok_or_else(|| StorageError::OverlayTemplateCreateFailed {
            path: template.to_path_buf(),
            err: std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "template path must have a parent directory",
            ),
        })?;
    if !parent.is_dir() {
        return Err(StorageError::OverlayTemplateCreateFailed {
            path: template.to_path_buf(),
            err: std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "template parent directory does not exist",
            ),
        });
    }

    let tmp = template.with_extension(format!("{}.tmp", std::process::id()));
    let _ = std::fs::remove_file(&tmp);
    let file = File::create(&tmp).map_err(|e| StorageError::OverlayTemplateCreateFailed {
        path: tmp.clone(),
        err: e,
    })?;
    file.set_len(size_bytes)
        .map_err(|e| StorageError::OverlayTemplateCreateFailed {
            path: tmp.clone(),
            err: e,
        })?;
    drop(file);

    if let Err(err) = mkfs_ext4(&tmp) {
        let _ = std::fs::remove_file(&tmp);
        return Err(err);
    }
    if let Err(err) = dig_template_holes(&tmp) {
        let _ = std::fs::remove_file(&tmp);
        return Err(err);
    }

    std::fs::rename(&tmp, template).map_err(|e| StorageError::OverlayTemplateCreateFailed {
        path: template.to_path_buf(),
        err: e,
    })?;
    write_template_metadata(template, size_bytes)
}

fn mkfs_ext4(path: &Path) -> Result<(), StorageError> {
    let out = Command::new("mkfs.ext4")
        .arg("-F")
        .arg(path)
        .output()
        .map_err(|e| StorageError::OverlayCreateFailed {
            path: path.to_path_buf(),
            err: e,
        })?;

    if !out.status.success() {
        return Err(StorageError::SubprocessFailed {
            program: "mkfs.ext4",
            path: path.to_path_buf(),
            status: format_exit(out.status),
            stderr: String::from_utf8_lossy(&out.stderr).trim().to_owned(),
        });
    }
    Ok(())
}

fn dig_template_holes(path: &Path) -> Result<(), StorageError> {
    let out = Command::new("fallocate")
        .arg("-d")
        .arg(path)
        .output()
        .map_err(|e| StorageError::OverlayTemplateCreateFailed {
            path: path.to_path_buf(),
            err: e,
        })?;

    if out.status.success() {
        return Ok(());
    }
    Err(StorageError::SubprocessFailed {
        program: "fallocate",
        path: path.to_path_buf(),
        status: format_exit(out.status),
        stderr: String::from_utf8_lossy(&out.stderr).trim().to_owned(),
    })
}

fn template_metadata_path(template: &Path) -> PathBuf {
    template.with_extension("meta")
}

fn expected_template_metadata(size_bytes: u64) -> String {
    format!(
        "schema_version={TEMPLATE_SCHEMA_VERSION}\nfs=ext4\nsize_bytes={size_bytes}\nmkfs=mkfs.ext4 -F\npostprocess=fallocate -d\n"
    )
}

fn write_template_metadata(template: &Path, size_bytes: u64) -> Result<(), StorageError> {
    let meta = template_metadata_path(template);
    let expected = expected_template_metadata(size_bytes);
    let mut file = File::create(&meta).map_err(|e| StorageError::OverlayTemplateCreateFailed {
        path: meta.clone(),
        err: e,
    })?;
    file.write_all(expected.as_bytes())
        .map_err(|e| StorageError::OverlayTemplateCreateFailed { path: meta, err: e })
}

fn validate_template(template: &Path, size_bytes: u64) -> Result<(), StorageError> {
    let actual_len = std::fs::metadata(template)
        .map_err(|e| StorageError::OverlayTemplateCreateFailed {
            path: template.to_path_buf(),
            err: e,
        })?
        .len();
    if actual_len != size_bytes {
        return Err(StorageError::OverlayTemplateMismatch {
            path: template.to_path_buf(),
            reason: format!("template size {actual_len} != requested size {size_bytes}"),
        });
    }

    let meta = template_metadata_path(template);
    let mut actual = String::new();
    File::open(&meta)
        .map_err(|e| StorageError::Io { path: meta.clone(), source: e })?
        .read_to_string(&mut actual)
        .map_err(|e| StorageError::Io { path: meta.clone(), source: e })?;
    let expected = expected_template_metadata(size_bytes);
    if actual != expected {
        return Err(StorageError::OverlayTemplateMismatch {
            path: meta,
            reason: "metadata content differs from requested template identity".to_string(),
        });
    }
    Ok(())
}

fn clone_template(template: &Path, dest: &Path) -> Result<(), StorageError> {
    let out = Command::new("cp")
        .args(TEMPLATE_CLONE_ARGS)
        .arg(template)
        .arg(dest)
        .output()
        .map_err(|e| StorageError::OverlayTemplateCloneFailed {
            template: template.to_path_buf(),
            dest: dest.to_path_buf(),
            err: e,
        })?;
    if out.status.success() {
        return Ok(());
    }
    let _ = std::fs::remove_file(dest);
    Err(StorageError::SubprocessFailed {
        program: "cp",
        path: dest.to_path_buf(),
        status: format_exit(out.status),
        stderr: String::from_utf8_lossy(&out.stderr).trim().to_owned(),
    })
}

struct TemplateLock {
    path: PathBuf,
}

impl TemplateLock {
    fn acquire(path: &Path) -> Result<Self, StorageError> {
        let start = Instant::now();
        loop {
            match OpenOptions::new().write(true).create_new(true).open(path) {
                Ok(_) => {
                    return Ok(Self {
                        path: path.to_path_buf(),
                    })
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    if start.elapsed() >= TEMPLATE_LOCK_TIMEOUT {
                        return Err(StorageError::OverlayTemplateCreateFailed {
                            path: path.to_path_buf(),
                            err: std::io::Error::new(
                                std::io::ErrorKind::TimedOut,
                                "timed out waiting for overlay template lock",
                            ),
                        });
                    }
                    thread::sleep(Duration::from_millis(25));
                }
                Err(e) => {
                    return Err(StorageError::OverlayTemplateCreateFailed {
                        path: path.to_path_buf(),
                        err: e,
                    })
                }
            }
        }
    }
}

impl Drop for TemplateLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::TEMPLATE_CLONE_ARGS;

    #[test]
    fn template_clone_command_requests_reflink_auto_and_sparse_fallback() {
        assert_eq!(TEMPLATE_CLONE_ARGS, &["--reflink=auto", "--sparse=always"]);
    }
}
