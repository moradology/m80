//! Scratch ext4 image creation, hydration, and post-stop change extraction.
//!
//! ## Extraction approach (v0.1 departure from predecessor)
//!
//! predecessor uses `debugfs rdump` to enumerate and extract files without a
//! remount.  v0.1 uses a loop-mount instead: simpler, produces identical
//! observable behaviour for the scratch image sizes we care about (~64 MiB–
//! 512 MiB), and avoids the non-trivial `debugfs` output-parsing surface.
//! A future revision can add the debugfs path for very large images.

use std::fs::{self, OpenOptions};
use std::os::unix::fs::FileTypeExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

use crate::{ChangeSet, Rejection, RejectionReason, StorageError, io_err};

/// A per-VM scratch ext4 image.
///
/// Created via [`Scratch::create`], extracted via [`Scratch::extract`].
#[derive(Debug)]
pub struct Scratch {
    path: PathBuf,
}

impl Scratch {
    /// Format a scratch ext4 image at `image` of `size` bytes and hydrate it
    /// from the host workspace tree at `workspace`.
    ///
    /// Steps:
    /// 1. Truncate-create `image` at `size` bytes.
    /// 2. `mkfs.ext4 -F image`.
    /// 3. Loop-mount to a temp directory.
    /// 4. Copy workspace contents (dirs + regular files; symlinks/specials
    ///    return [`StorageError::AdmissibilityRefused`]).
    /// 5. Unmount.
    pub fn create(workspace: &Path, image: &Path, size: u64) -> Result<Self, StorageError> {
        match do_create(workspace, image, size) {
            Ok(()) => Ok(Self { path: image.to_path_buf() }),
            Err(e) => {
                // Best-effort: remove the partially-formatted image so the
                // caller doesn't have to clean it up.
                let _ = fs::remove_file(image);
                Err(e)
            }
        }
    }

    /// Post-stop extraction: `e2fsck` → loop-mount (read-only) → admissibility
    /// scan → staging → atomic rename into `into`.
    ///
    /// Failure at any step leaves `into` unchanged.
    ///
    /// Returns a [`ChangeSet`] describing what was staged and what was
    /// rejected.
    pub fn extract(image: &Path, into: &Path) -> Result<ChangeSet, StorageError> {
        // 1. e2fsck -p -f (preen + force-check even if clean).
        run_e2fsck(image)?;

        // 2. Loop-mount read-only.
        let mount_dir = TempDir::new().map_err(|e| io_err(image, e))?;
        mount_loop_ro(image, mount_dir.path())?;

        // 3 + 4 + 5: walk, scan admissibility, stage into a sibling temp dir.
        let stage_result = build_stage(mount_dir.path());

        // 6. Unmount before the rename. The staging tree lives in a sibling
        //    `TempDir`, so it's not on the now-unmounted filesystem.
        umount(mount_dir.path())?;

        let (stage_dir, change_set) = stage_result?;

        // 7. Atomic rename into `into`; fail if it already exists.
        if into.exists() {
            return Err(StorageError::SwapFailed);
        }
        fs::rename(stage_dir.path(), into).map_err(|_| StorageError::SwapFailed)?;
        // Prevent TempDir from trying to remove the path we just renamed away.
        let _ = stage_dir.keep();

        Ok(change_set)
    }

    /// Path of this scratch image.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Inner pipeline for `Scratch::create`. Outer wrapper removes the image on
/// any failure so callers don't have to clean up partial state.
fn do_create(workspace: &Path, image: &Path, size: u64) -> Result<(), StorageError> {
    // 1. Truncate-create the image file.
    let file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(image)
        .map_err(|e| io_err(image, e))?;
    file.set_len(size).map_err(|e| io_err(image, e))?;
    drop(file);

    // 2. mkfs.ext4 -F image
    let mkfs_out = Command::new("mkfs.ext4")
        .args(["-F", &image.display().to_string()])
        .output()
        .map_err(StorageError::Mkfs)?;
    if !mkfs_out.status.success() {
        let stderr = String::from_utf8_lossy(&mkfs_out.stderr).trim().to_owned();
        let stdout = String::from_utf8_lossy(&mkfs_out.stdout).trim().to_owned();
        return Err(StorageError::Mkfs(io::mkfs_error(if stderr.is_empty() {
            stdout
        } else {
            stderr
        })));
    }

    // 3. Loop-mount to temp dir.
    let mount_dir = TempDir::new().map_err(|e| io_err(image, e))?;
    mount_loop(image, mount_dir.path())?;

    // 4. Copy workspace contents; always umount before returning. The copy
    //    error wins over the umount error if both fail (the user wants to
    //    know what went wrong with their workspace, not that umount also
    //    couldn't recover).
    let copy_result = copy_workspace_into(workspace, mount_dir.path());
    let umount_result = umount(mount_dir.path());
    copy_result?;
    umount_result
}

// ---------------------------------------------------------------------------
// e2fsck
// ---------------------------------------------------------------------------

/// e2fsck exit-code acceptability.
///
/// Bits 0 and 1 mean "errors found/corrected"; bit 2+ are fatal.
/// We accept 0..=3 (the filesystem is usable after the run).
fn e2fsck_exit_acceptable(code: Option<i32>) -> bool {
    code.is_some_and(|c| c & !0b11 == 0)
}

fn run_e2fsck(image: &Path) -> Result<(), StorageError> {
    let out = Command::new("e2fsck")
        .args(["-p", "-f", &image.display().to_string()])
        .output()
        .map_err(|e| io_err(image, e))?;

    if e2fsck_exit_acceptable(out.status.code()) {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_owned();
    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    Err(StorageError::E2fsckFailed {
        exit: out.status.code().unwrap_or(-1),
        stderr: if stderr.is_empty() { stdout } else { stderr },
    })
}

// ---------------------------------------------------------------------------
// Mount / umount helpers
// ---------------------------------------------------------------------------

fn mount_loop(image: &Path, mount_point: &Path) -> Result<(), StorageError> {
    let out = Command::new("mount")
        .args([
            "-o",
            "loop",
            &image.display().to_string(),
            &mount_point.display().to_string(),
        ])
        .output()
        .map_err(|e| io_err(mount_point, e))?;
    if !out.status.success() {
        return Err(io_err(
            mount_point,
            std::io::Error::other(format!(
                "mount failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            )),
        ));
    }
    Ok(())
}

fn mount_loop_ro(image: &Path, mount_point: &Path) -> Result<(), StorageError> {
    let out = Command::new("mount")
        .args([
            "-o",
            "loop,ro",
            &image.display().to_string(),
            &mount_point.display().to_string(),
        ])
        .output()
        .map_err(|e| io_err(mount_point, e))?;
    if !out.status.success() {
        return Err(io_err(
            mount_point,
            std::io::Error::other(format!(
                "mount (ro) failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            )),
        ));
    }
    Ok(())
}

fn umount(mount_point: &Path) -> Result<(), StorageError> {
    let out = Command::new("umount")
        .arg(mount_point.display().to_string())
        .output()
        .map_err(|e| io_err(mount_point, e))?;
    if !out.status.success() {
        return Err(io_err(
            mount_point,
            std::io::Error::other(format!(
                "umount failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            )),
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Workspace copy (hydration)
// ---------------------------------------------------------------------------

/// Recursively copy `workspace` contents into `dest`.
///
/// Directories and regular files are copied; symlinks/specials return
/// [`StorageError::AdmissibilityRefused`].
fn copy_workspace_into(workspace: &Path, dest: &Path) -> Result<(), StorageError> {
    copy_tree(workspace, workspace, dest)
}

fn copy_tree(root: &Path, src: &Path, dst_root: &Path) -> Result<(), StorageError> {
    for entry in fs::read_dir(src).map_err(|e| io_err(src, e))? {
        let entry = entry.map_err(|e| io_err(src, e))?;
        let src_path = entry.path();
        let meta = fs::symlink_metadata(&src_path).map_err(|e| io_err(&src_path, e))?;
        let ft = meta.file_type();

        let rel = src_path
            .strip_prefix(root)
            .expect("entry always under root");
        let dst_path = dst_root.join(rel);

        if ft.is_symlink() || ft.is_fifo() || ft.is_socket() || ft.is_block_device() || ft.is_char_device() {
            return Err(StorageError::AdmissibilityRefused);
        }

        if ft.is_dir() {
            fs::create_dir(&dst_path).map_err(|e| io_err(&dst_path, e))?;
            copy_tree(root, &src_path, dst_root)?;
        } else if ft.is_file() {
            fs::copy(&src_path, &dst_path).map_err(|e| io_err(&src_path, e))?;
        } else {
            return Err(StorageError::AdmissibilityRefused);
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Extraction: build staging tree
// ---------------------------------------------------------------------------

/// Walk `mount_root`, apply admissibility scan, copy survivors into a
/// temporary staging directory.
///
/// Returns `(stage_dir, ChangeSet)`.
fn build_stage(mount_root: &Path) -> Result<(TempDir, ChangeSet), StorageError> {
    let stage = TempDir::new().map_err(|e| io_err(mount_root, e))?;
    let mut staged: Vec<PathBuf> = Vec::new();
    let mut rejected: Vec<Rejection> = Vec::new();
    let mut total_bytes: u64 = 0;

    walk_for_extract(
        mount_root,
        mount_root,
        stage.path(),
        &mut staged,
        &mut rejected,
        &mut total_bytes,
    )?;

    Ok((
        stage,
        ChangeSet {
            staged,
            rejected,
            total_bytes,
        },
    ))
}

fn walk_for_extract(
    mount_root: &Path,
    src: &Path,
    dst_root: &Path,
    staged: &mut Vec<PathBuf>,
    rejected: &mut Vec<Rejection>,
    total_bytes: &mut u64,
) -> Result<(), StorageError> {
    for entry in fs::read_dir(src).map_err(|e| io_err(src, e))? {
        let entry = entry.map_err(|e| io_err(src, e))?;
        let src_path = entry.path();
        let meta = fs::symlink_metadata(&src_path).map_err(|e| io_err(&src_path, e))?;
        let ft = meta.file_type();

        let rel = src_path
            .strip_prefix(mount_root)
            .expect("entry always under mount_root");

        // Skip lost+found (ext4 artefact).
        if rel == Path::new("lost+found") {
            continue;
        }

        let reason = if ft.is_symlink() {
            Some(RejectionReason::Symlink)
        } else if ft.is_fifo() || ft.is_socket() || ft.is_block_device() || ft.is_char_device() {
            Some(RejectionReason::SpecialFile)
        } else {
            None
        };

        if let Some(r) = reason {
            rejected.push(Rejection {
                path: rel.to_path_buf(),
                reason: r,
            });
            continue;
        }

        let dst_path = dst_root.join(rel);

        if ft.is_dir() {
            fs::create_dir(&dst_path).map_err(|e| io_err(&dst_path, e))?;
            staged.push(rel.to_path_buf());
            walk_for_extract(mount_root, &src_path, dst_root, staged, rejected, total_bytes)?;
        } else if ft.is_file() {
            fs::copy(&src_path, &dst_path).map_err(|e| io_err(&src_path, e))?;
            *total_bytes = total_bytes.saturating_add(meta.len());
            staged.push(rel.to_path_buf());
        } else {
            rejected.push(Rejection {
                path: rel.to_path_buf(),
                reason: RejectionReason::Other("unsupported file type".into()),
            });
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Small helper to construct io::Error for mkfs failures
// ---------------------------------------------------------------------------

mod io {
    pub(crate) fn mkfs_error(msg: impl Into<String>) -> std::io::Error {
        std::io::Error::other(msg.into())
    }
}

