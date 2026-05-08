//! Scratch ext4 image creation, hydration, and post-stop change extraction.
//! Loop-mounts the image to walk it; `debugfs rdump` would also work but
//! the parser surface isn't worth it at the scratch sizes we target.

use std::fs::{self, OpenOptions};
use std::os::unix::fs::FileTypeExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

use crate::{format_exit, io_err, ChangeSet, Rejection, RejectionReason, StorageError};

/// Minimum scratch image size: 64 MiB.
pub(crate) const MIN_SCRATCH_BYTES: u64 = 64 * 1024 * 1024;
/// Extra headroom added above the host workspace's current file bytes.
pub(crate) const SCRATCH_PADDING_BYTES: u64 = 32 * 1024 * 1024;
/// Scratch image sizes are rounded up to a 4 MiB boundary.
pub(crate) const SCRATCH_ALIGNMENT_BYTES: u64 = 4 * 1024 * 1024;

/// A per-VM scratch ext4 image.
///
/// Created via [`Scratch::create`], extracted via [`Scratch::extract`].
#[derive(Debug)]
pub struct Scratch {
    path: PathBuf,
}

impl Scratch {
    /// Recommended scratch image size for `workspace`.
    ///
    /// Directory entries are walked recursively. Regular file lengths are
    /// counted; symlinks and special files return
    /// [`StorageError::AdmissibilityRefused`], matching hydration.
    ///
    /// The rule is `max(64 MiB, used_bytes + 32 MiB)`, rounded up to a
    /// 4 MiB boundary.
    pub fn recommended_size_for_workspace(workspace: &Path) -> Result<u64, StorageError> {
        let used = workspace_used_bytes(workspace)?;
        let padded = used.saturating_add(SCRATCH_PADDING_BYTES);
        Ok(align_scratch_size(padded.max(MIN_SCRATCH_BYTES)))
    }

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
            Ok(()) => Ok(Self {
                path: image.to_path_buf(),
            }),
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
        if into.exists() {
            return Err(StorageError::SwapFailed);
        }

        run_e2fsck(image)?;

        let mount_dir = TempDir::new().map_err(|e| io_err(image, e))?;
        run_mount(mount_dir.path(), &["mount", "-o", "loop,ro"], Some(image))?;

        let stage_result = build_stage(mount_dir.path(), into);

        // Unmount before the rename; the staging tree lives in a sibling
        // TempDir, so it's not on the now-unmounted filesystem.
        run_mount(mount_dir.path(), &["umount"], None)?;

        let (stage_dir, change_set) = stage_result?;

        // Atomic rename into `into`; fail if it already exists or if the
        // sibling-stage invariant was broken.
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

fn align_scratch_size(size: u64) -> u64 {
    let remainder = size % SCRATCH_ALIGNMENT_BYTES;
    if remainder == 0 {
        size
    } else {
        size.saturating_add(SCRATCH_ALIGNMENT_BYTES - remainder)
    }
}

fn workspace_used_bytes(src: &Path) -> Result<u64, StorageError> {
    let mut used = 0_u64;
    for entry in fs::read_dir(src).map_err(|e| io_err(src, e))? {
        let entry = entry.map_err(|e| io_err(src, e))?;
        let src_path = entry.path();
        let meta = fs::symlink_metadata(&src_path).map_err(|e| io_err(&src_path, e))?;
        let ft = meta.file_type();

        if ft.is_symlink()
            || ft.is_fifo()
            || ft.is_socket()
            || ft.is_block_device()
            || ft.is_char_device()
        {
            return Err(StorageError::AdmissibilityRefused);
        }

        if ft.is_dir() {
            used = used.saturating_add(workspace_used_bytes(&src_path)?);
        } else if ft.is_file() {
            used = used.saturating_add(meta.len());
        } else {
            return Err(StorageError::AdmissibilityRefused);
        }
    }
    Ok(used)
}

/// Inner pipeline for `Scratch::create`. Outer wrapper removes the image on
/// any failure so callers don't have to clean up partial state.
fn do_create(workspace: &Path, image: &Path, size: u64) -> Result<(), StorageError> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(image)
        .map_err(|e| io_err(image, e))?;
    file.set_len(size).map_err(|e| io_err(image, e))?;
    drop(file);

    let mkfs_out = Command::new("mkfs.ext4")
        .arg("-F")
        .arg(image)
        .output()
        .map_err(|e| io_err(image, e))?;
    if !mkfs_out.status.success() {
        let detail = if mkfs_out.stderr.is_empty() {
            String::from_utf8_lossy(&mkfs_out.stdout).trim().to_owned()
        } else {
            String::from_utf8_lossy(&mkfs_out.stderr).trim().to_owned()
        };
        return Err(StorageError::SubprocessFailed {
            program: "mkfs.ext4",
            path: image.to_path_buf(),
            status: format_exit(mkfs_out.status),
            stderr: detail,
        });
    }

    let mount_dir = TempDir::new().map_err(|e| io_err(image, e))?;
    run_mount(mount_dir.path(), &["mount", "-o", "loop"], Some(image))?;

    // Copy workspace contents; always umount before returning. The copy
    // error wins over the umount error if both fail (the user wants to
    // know what went wrong with their workspace, not that umount also
    // couldn't recover).
    let copy_result = copy_workspace_into(workspace, mount_dir.path());
    let umount_result = run_mount(mount_dir.path(), &["umount"], None);
    copy_result?;
    umount_result
}

/// e2fsck exit-code acceptability.
///
/// Bits 0 and 1 mean "errors found/corrected"; bit 2+ are fatal.
/// We accept 0..=3 (the filesystem is usable after the run).
fn e2fsck_exit_acceptable(code: Option<i32>) -> bool {
    code.is_some_and(|c| c & !0b11 == 0)
}

fn run_e2fsck(image: &Path) -> Result<(), StorageError> {
    let out = Command::new("e2fsck")
        .args(["-p", "-f"])
        .arg(image)
        .output()
        .map_err(|e| io_err(image, e))?;

    if e2fsck_exit_acceptable(out.status.code()) {
        return Ok(());
    }

    let detail = if out.stderr.is_empty() {
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    } else {
        String::from_utf8_lossy(&out.stderr).trim().to_owned()
    };
    Err(StorageError::SubprocessFailed {
        program: "e2fsck",
        path: image.to_path_buf(),
        status: format_exit(out.status),
        stderr: detail,
    })
}

/// Spawn `argv[0]` with `argv[1..]` followed by an optional `image` path
/// and `mount_point`; surface a non-zero exit's stderr in the error.
fn run_mount(mount_point: &Path, argv: &[&str], image: Option<&Path>) -> Result<(), StorageError> {
    let (program, args) = argv.split_first().expect("argv non-empty");
    let mut cmd = Command::new(program);
    cmd.args(args);
    if let Some(img) = image {
        cmd.arg(img);
    }
    cmd.arg(mount_point);
    let out = cmd.output().map_err(|e| io_err(mount_point, e))?;
    if !out.status.success() {
        let detail = if out.stderr.is_empty() {
            String::from_utf8_lossy(&out.stdout).trim().to_owned()
        } else {
            String::from_utf8_lossy(&out.stderr).trim().to_owned()
        };
        return Err(io_err(
            mount_point,
            std::io::Error::other(format!("{program} failed: {detail}")),
        ));
    }
    Ok(())
}

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

        if ft.is_symlink()
            || ft.is_fifo()
            || ft.is_socket()
            || ft.is_block_device()
            || ft.is_char_device()
        {
            return Err(StorageError::AdmissibilityRefused);
        }

        if ft.is_dir() {
            fs::create_dir(&dst_path).map_err(|e| io_err(&dst_path, e))?;
            fs::set_permissions(&dst_path, meta.permissions()).map_err(|e| io_err(&dst_path, e))?;
            copy_tree(root, &src_path, dst_root)?;
        } else if ft.is_file() {
            fs::copy(&src_path, &dst_path).map_err(|e| io_err(&src_path, e))?;
        } else {
            return Err(StorageError::AdmissibilityRefused);
        }
    }
    Ok(())
}

/// Walk `mount_root`, apply admissibility scan, copy survivors into a
/// temporary staging directory.
///
/// Returns `(stage_dir, ChangeSet)`.
fn build_stage(mount_root: &Path, into: &Path) -> Result<(TempDir, ChangeSet), StorageError> {
    let stage_parent = stage_parent(into);
    let stage = tempfile::Builder::new()
        .prefix(&stage_prefix(into))
        .tempdir_in(&stage_parent)
        .map_err(|e| io_err(&stage_parent, e))?;
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

fn stage_parent(into: &Path) -> PathBuf {
    into.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf()
}

fn stage_prefix(into: &Path) -> String {
    let file_name = into
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("workspace");
    format!(".{file_name}.m80-writeback-stage-{}-", std::process::id())
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
            fs::set_permissions(&dst_path, meta.permissions()).map_err(|e| io_err(&dst_path, e))?;
            staged.push(rel.to_path_buf());
            walk_for_extract(
                mount_root,
                &src_path,
                dst_root,
                staged,
                rejected,
                total_bytes,
            )?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_prefix_names_sibling_writeback_stage() {
        let into = Path::new("/tmp/workspace");
        let prefix = stage_prefix(into);
        assert!(prefix.starts_with(".workspace.m80-writeback-stage-"));
    }

    #[test]
    fn stage_parent_is_destination_parent() {
        let into = Path::new("/tmp/m80/ws");
        assert_eq!(stage_parent(into), PathBuf::from("/tmp/m80"));
    }

    #[test]
    fn relative_stage_parent_defaults_to_current_directory() {
        assert_eq!(stage_parent(Path::new("ws")), PathBuf::from("."));
    }
}
