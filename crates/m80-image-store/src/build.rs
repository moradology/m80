//! Minimal local-dev image builders.

use std::path::Path;
use std::process::Command;

use crate::{ImageKind, StoreError};

pub(crate) fn build_image(
    source_dir: &Path,
    output: &Path,
    kind: ImageKind,
) -> Result<(), StoreError> {
    match kind {
        ImageKind::Erofs => build_erofs(source_dir, output),
        ImageKind::Ext4 => build_ext4(source_dir, output),
    }
}

fn build_erofs(source_dir: &Path, output: &Path) -> Result<(), StoreError> {
    let output = Command::new("mkfs.erofs")
        .arg("--quiet")
        .arg("-T")
        .arg("0")
        .arg("--all-root")
        .arg("--force-uid=0")
        .arg("--force-gid=0")
        .arg("-U")
        .arg("00000000-0000-0000-0000-000000000000")
        .arg(output)
        .arg(source_dir)
        .output()
        .map_err(|err| StoreError::MkfsFailed {
            kind: ImageKind::Erofs,
            detail: err.to_string(),
        })?;
    check_status(ImageKind::Erofs, output)
}

fn build_ext4(source_dir: &Path, output: &Path) -> Result<(), StoreError> {
    let file = std::fs::File::create(output).map_err(|source| StoreError::Io {
        path: output.to_path_buf(),
        source,
    })?;
    file.set_len(16 * 1024 * 1024)
        .map_err(|source| StoreError::Io {
            path: output.to_path_buf(),
            source,
        })?;
    drop(file);

    let output = Command::new("mkfs.ext4")
        .arg("-F")
        .arg("-q")
        .arg("-U")
        .arg("00000000-0000-0000-0000-000000000000")
        .arg("-E")
        .arg("root_owner=0:0,lazy_itable_init=0,lazy_journal_init=0")
        .arg("-d")
        .arg(source_dir)
        .arg(output)
        .output()
        .map_err(|err| StoreError::MkfsFailed {
            kind: ImageKind::Ext4,
            detail: err.to_string(),
        })?;
    check_status(ImageKind::Ext4, output)
}

fn check_status(kind: ImageKind, output: std::process::Output) -> Result<(), StoreError> {
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    let detail = if stderr.is_empty() {
        format!("helper exited with {}", output.status)
    } else {
        stderr
    };
    Err(StoreError::MkfsFailed { kind, detail })
}
