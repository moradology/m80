use std::fs;
use std::io;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::Command;

use m80_firecracker::{ConfigError, FcError};

#[derive(Debug)]
pub(super) struct PathHandoffSummary {
    pub(super) installed_m80_path: String,
    pub(super) installed_m80_version: String,
    link_path: PathBuf,
    backup_link: PathBuf,
    had_previous_link: bool,
    mutated: bool,
}

pub(super) fn install_path_handoff(
    final_dir: &Path,
    bin_dir: &Path,
) -> Result<PathHandoffSummary, FcError> {
    fs::create_dir_all(bin_dir).map_err(|source| FcError::PathIo {
        path: bin_dir.to_path_buf(),
        source,
    })?;
    let installed_m80 = final_dir.join("bin/m80");
    let link_path = bin_dir.join("m80");
    if final_dir
        .parent()
        .and_then(Path::parent)
        .is_some_and(|install_root| bin_dir == install_root.join("bin"))
    {
        let installed_m80_version = verify_path_handoff(&link_path, bin_dir)?;
        return Ok(PathHandoffSummary {
            installed_m80_path: link_path.display().to_string(),
            installed_m80_version,
            link_path,
            backup_link: bin_dir.join(format!(".m80.install.previous.{}", std::process::id())),
            had_previous_link: false,
            mutated: false,
        });
    }
    let temp_link = bin_dir.join(format!(".m80.install.{}", std::process::id()));
    let backup_link = bin_dir.join(format!(".m80.install.previous.{}", std::process::id()));
    if temp_link.exists() {
        fs::remove_file(&temp_link).map_err(|source| FcError::PathIo {
            path: temp_link.clone(),
            source,
        })?;
    }
    if backup_link.exists() {
        fs::remove_file(&backup_link).map_err(|source| FcError::PathIo {
            path: backup_link.clone(),
            source,
        })?;
    }
    let previous_link = previous_m80_link_state(&link_path)?;
    symlink(&installed_m80, &temp_link).map_err(|source| FcError::PathIo {
        path: temp_link.clone(),
        source,
    })?;
    if previous_link.exists {
        fs::rename(&link_path, &backup_link).map_err(|source| FcError::PathIo {
            path: link_path.clone(),
            source,
        })?;
    }
    fs::rename(&temp_link, &link_path).map_err(|source| FcError::PathIo {
        path: link_path.clone(),
        source,
    })?;

    let handoff = verify_path_handoff(&link_path, bin_dir);
    if handoff.is_err() {
        restore_previous_m80_link(&link_path, &backup_link, previous_link.exists)?;
    }
    let installed_m80_version = handoff?;
    Ok(PathHandoffSummary {
        installed_m80_path: link_path.display().to_string(),
        installed_m80_version,
        link_path,
        backup_link,
        had_previous_link: previous_link.exists,
        mutated: true,
    })
}

impl PathHandoffSummary {
    pub(super) fn rollback(&self) -> Result<(), FcError> {
        if !self.mutated {
            return Ok(());
        }
        restore_previous_m80_link(&self.link_path, &self.backup_link, self.had_previous_link)
    }

    pub(super) fn commit(&self) {
        if self.mutated && self.had_previous_link {
            let _ = fs::remove_file(&self.backup_link);
        }
    }
}

pub(super) fn path_handoff_rollback_error(primary: FcError, rollback: FcError) -> FcError {
    FcError::Config(ConfigError::InvalidValue {
        field: "install.bin_dir",
        reason: format!(
            "active pointer flip failed after PATH handoff, and PATH handoff rollback also failed; primary_error={primary}; rollback_error={rollback}"
        ),
    })
}

#[derive(Debug)]
struct PreviousM80Link {
    exists: bool,
}

fn previous_m80_link_state(link_path: &Path) -> Result<PreviousM80Link, FcError> {
    match link_path.symlink_metadata() {
        Ok(metadata) => {
            let kind = metadata.file_type();
            if kind.is_file() || kind.is_symlink() {
                Ok(PreviousM80Link { exists: true })
            } else {
                Err(FcError::Config(ConfigError::InvalidValue {
                    field: "install.bin_dir",
                    reason: format!(
                        "{} already exists but is not a file or symlink",
                        link_path.display()
                    ),
                }))
            }
        }
        Err(source) if source.kind() == io::ErrorKind::NotFound => {
            Ok(PreviousM80Link { exists: false })
        }
        Err(source) => Err(FcError::PathIo {
            path: link_path.to_path_buf(),
            source,
        }),
    }
}

fn verify_path_handoff(link_path: &Path, bin_dir: &Path) -> Result<String, FcError> {
    match command_v_m80()? {
        Some(resolved) if resolved == link_path => installed_m80_version(link_path),
        Some(resolved) => Err(path_handoff_error(bin_dir, link_path, Some(&resolved))),
        None => Err(path_handoff_error(bin_dir, link_path, None)),
    }
}

fn path_handoff_error(bin_dir: &Path, link_path: &Path, resolved: Option<&Path>) -> FcError {
    let observed = resolved
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "no m80 on PATH".to_owned());
    FcError::Config(ConfigError::InvalidValue {
        field: "install.bin_dir",
        reason: format!(
            "PATH handoff failed: command -v m80 resolved {observed} but expected {}; repair with: export PATH={}:$PATH",
            link_path.display(),
            bin_dir.display()
        ),
    })
}

fn command_v_m80() -> Result<Option<PathBuf>, FcError> {
    let output = Command::new("sh")
        .arg("-c")
        .arg("command -v m80")
        .output()
        .map_err(|source| FcError::CommandSpawnFailed {
            command: "resolve installed m80",
            source,
        })?;
    if !output.status.success() {
        return Ok(None);
    }
    Ok(Some(PathBuf::from(
        String::from_utf8_lossy(&output.stdout).trim().to_owned(),
    )))
}

fn installed_m80_version(path: &Path) -> Result<String, FcError> {
    let output = Command::new(path)
        .arg("--version")
        .output()
        .map_err(|source| FcError::CommandSpawnFailed {
            command: "installed m80 --version",
            source,
        })?;
    if !output.status.success() {
        return Err(FcError::CommandFailed {
            command: "installed m80 --version",
            status: output.status,
            output: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn restore_previous_m80_link(
    link_path: &Path,
    backup_link: &Path,
    had_previous_link: bool,
) -> Result<(), FcError> {
    match fs::remove_file(link_path) {
        Ok(()) => {}
        Err(source) if source.kind() == io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(FcError::PathIo {
                path: link_path.to_path_buf(),
                source,
            });
        }
    }
    if had_previous_link {
        fs::rename(backup_link, link_path).map_err(|source| FcError::PathIo {
            path: link_path.to_path_buf(),
            source,
        })?;
    }
    Ok(())
}
