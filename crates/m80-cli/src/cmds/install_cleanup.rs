use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};

use m80_firecracker::{ConfigError, FcError};
use serde::Serialize;

use crate::args::InstallCleanupArgs;
use crate::{errors, json};

const VERSIONS_DIR: &str = "versions";

pub(super) fn cmd_install_cleanup(
    args: InstallCleanupArgs,
    json_mode: bool,
) -> anyhow::Result<i32> {
    match cleanup_version(&args) {
        Ok(summary) => {
            render_summary(&summary, json_mode);
            Ok(0)
        }
        Err(err) => Ok(errors::render_error(&err, json_mode)),
    }
}

#[derive(Debug, Serialize)]
struct InstallCleanupSummary {
    status: &'static str,
    install_root: String,
    release_tag: String,
    removed_version_dir: String,
    active_pointer: String,
    active_pointer_removed: bool,
}

fn cleanup_version(args: &InstallCleanupArgs) -> Result<InstallCleanupSummary, FcError> {
    let install_root = normalize_install_root(&args.install_root)?;
    validate_release_tag(&args.release_tag)?;
    let versions_dir = install_root.join(VERSIONS_DIR);
    let version_dir = versions_dir.join(&args.release_tag);
    validate_version_dir_shape(&install_root, &version_dir)?;
    let active_pointer = install_root.join("active");
    let active_state = active_pointer_state(&install_root, &active_pointer)?;
    let is_active = active_state.as_ref() == Some(&version_dir);
    if is_active && !args.remove_active {
        return Err(FcError::Config(ConfigError::InvalidValue {
            field: "install_cleanup.release_tag",
            reason: format!(
                "{} is the active installed version; rollback first or rerun with --remove-active",
                version_dir.display()
            ),
        }));
    }
    if is_active {
        remove_active_version(&active_pointer, &version_dir)?;
    } else {
        fs::remove_dir_all(&version_dir).map_err(|source| FcError::PathIo {
            path: version_dir.clone(),
            source,
        })?;
    }
    Ok(InstallCleanupSummary {
        status: "removed",
        install_root: install_root.display().to_string(),
        release_tag: args.release_tag.clone(),
        removed_version_dir: version_dir.display().to_string(),
        active_pointer: active_pointer.display().to_string(),
        active_pointer_removed: is_active,
    })
}

fn remove_active_version(active_pointer: &Path, version_dir: &Path) -> Result<(), FcError> {
    let backup_pointer =
        active_pointer.with_file_name(format!(".active.cleanup-backup-{}", std::process::id()));
    remove_file_if_exists(&backup_pointer)?;
    fs::rename(active_pointer, &backup_pointer).map_err(|source| FcError::PathIo {
        path: active_pointer.to_path_buf(),
        source,
    })?;
    match fs::remove_dir_all(version_dir) {
        Ok(()) => {
            remove_file_if_exists(&backup_pointer)?;
            Ok(())
        }
        Err(source) => {
            let cleanup_error = FcError::PathIo {
                path: version_dir.to_path_buf(),
                source,
            };
            let _ = fs::rename(&backup_pointer, active_pointer);
            Err(cleanup_error)
        }
    }
}

fn remove_file_if_exists(path: &Path) -> Result<(), FcError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(FcError::PathIo {
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn render_summary(summary: &InstallCleanupSummary, json_mode: bool) {
    if json_mode {
        println!("{}", json::to_pretty(summary));
        return;
    }
    if summary.active_pointer_removed {
        println!("removed active install version");
    } else {
        println!("removed inactive install version");
    }
    println!("install_root={}", summary.install_root);
    println!("release_tag={}", summary.release_tag);
    println!("removed_version_dir={}", summary.removed_version_dir);
    println!("active_pointer={}", summary.active_pointer);
    println!("active_pointer_removed={}", summary.active_pointer_removed);
}

fn normalize_install_root(path: &Path) -> Result<PathBuf, FcError> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|source| FcError::PathIo {
                path: PathBuf::from("."),
                source,
            })?
            .join(path)
    };
    reject_parent_components("install_cleanup.install_root", &absolute)?;
    let normalized = lexical_normalize(&absolute);
    reject_symlinked_install_root(&normalized)?;
    Ok(normalized)
}

fn validate_release_tag(tag: &str) -> Result<(), FcError> {
    if tag.is_empty()
        || tag.contains('/')
        || tag.contains('\\')
        || tag == "."
        || tag == ".."
        || tag.contains('\0')
    {
        return Err(FcError::Config(ConfigError::InvalidValue {
            field: "install_cleanup.release_tag",
            reason: format!("release tag is not a single version directory name: {tag:?}"),
        }));
    }
    Ok(())
}

fn validate_version_dir_shape(install_root: &Path, version_dir: &Path) -> Result<(), FcError> {
    let versions_dir = install_root.join(VERSIONS_DIR);
    let install_root_metadata =
        require_real_directory_metadata("install_cleanup.install_root", install_root)?;
    let versions_metadata =
        require_real_directory_metadata("install_cleanup.versions_dir", &versions_dir)?;
    require_owner(
        install_root,
        VERSIONS_DIR,
        &versions_metadata,
        install_root_metadata.uid(),
    )?;
    if version_dir.parent() != Some(versions_dir.as_path())
        || !version_dir.starts_with(&versions_dir)
    {
        return Err(FcError::Config(ConfigError::InvalidValue {
            field: "install_cleanup.version_dir",
            reason: format!(
                "version directory must be one child of {}, got {}",
                versions_dir.display(),
                version_dir.display()
            ),
        }));
    }
    let metadata = require_real_directory_metadata("install_cleanup.version_dir", version_dir)?;
    require_owner(
        &versions_dir,
        version_dir
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("<version>"),
        &metadata,
        versions_metadata.uid(),
    )?;
    require_regular_file(version_dir, "bundle.json", metadata.uid())?;
    require_regular_file(version_dir, "bin/m80", metadata.uid())?;
    require_directory(version_dir, "artifacts", metadata.uid())?;
    Ok(())
}

fn require_regular_file(
    version_dir: &Path,
    required: &str,
    expected_uid: u32,
) -> Result<(), FcError> {
    let required_path = version_dir.join(required);
    let metadata = required_metadata(version_dir, required, &required_path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(FcError::Config(ConfigError::InvalidValue {
            field: "install_cleanup.version_dir",
            reason: format!(
                "refusing to remove malformed version directory {}; {} is not a regular file",
                version_dir.display(),
                required
            ),
        }));
    }
    require_owner(version_dir, required, &metadata, expected_uid)?;
    Ok(())
}

fn require_directory(version_dir: &Path, required: &str, expected_uid: u32) -> Result<(), FcError> {
    let required_path = version_dir.join(required);
    let metadata = required_metadata(version_dir, required, &required_path)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(FcError::Config(ConfigError::InvalidValue {
            field: "install_cleanup.version_dir",
            reason: format!(
                "refusing to remove malformed version directory {}; {} is not a real directory",
                version_dir.display(),
                required
            ),
        }));
    }
    require_owner(version_dir, required, &metadata, expected_uid)?;
    Ok(())
}

fn require_real_directory_metadata(
    field: &'static str,
    path: &Path,
) -> Result<fs::Metadata, FcError> {
    let metadata = fs::symlink_metadata(path).map_err(|source| FcError::PathIo {
        path: path.to_path_buf(),
        source,
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(FcError::Config(ConfigError::InvalidValue {
            field,
            reason: format!("path must be a real directory: {}", path.display()),
        }));
    }
    Ok(metadata)
}

fn require_owner(
    base: &Path,
    entry: &str,
    metadata: &fs::Metadata,
    expected_uid: u32,
) -> Result<(), FcError> {
    if metadata.uid() != expected_uid {
        return Err(FcError::Config(ConfigError::InvalidValue {
            field: "install_cleanup.version_dir",
            reason: format!(
                "refusing to remove version directory with mixed ownership {}; {} has uid {}, expected {}",
                base.display(),
                entry,
                metadata.uid(),
                expected_uid
            ),
        }));
    }
    Ok(())
}

fn required_metadata(
    version_dir: &Path,
    required: &str,
    required_path: &Path,
) -> Result<fs::Metadata, FcError> {
    fs::symlink_metadata(required_path).map_err(|source| {
        if source.kind() == std::io::ErrorKind::NotFound {
            return FcError::Config(ConfigError::InvalidValue {
                field: "install_cleanup.version_dir",
                reason: format!(
                    "refusing to remove malformed version directory {}; missing {}",
                    version_dir.display(),
                    required
                ),
            });
        }
        FcError::PathIo {
            path: required_path.to_path_buf(),
            source,
        }
    })
}

fn active_pointer_state(
    install_root: &Path,
    active_pointer: &Path,
) -> Result<Option<PathBuf>, FcError> {
    match fs::read_link(active_pointer) {
        Ok(target) => {
            if !target.is_absolute() || has_parent_component(&target) {
                return Err(FcError::Config(ConfigError::InvalidValue {
                    field: "install_cleanup.active_pointer",
                    reason: format!("active pointer target is unsafe: {}", target.display()),
                }));
            }
            let versions_dir = install_root.join(VERSIONS_DIR);
            if target.parent() != Some(versions_dir.as_path()) || !target.starts_with(&versions_dir)
            {
                return Err(FcError::Config(ConfigError::InvalidValue {
                    field: "install_cleanup.active_pointer",
                    reason: format!(
                        "active pointer target must be one version directory under {}, got {}",
                        versions_dir.display(),
                        target.display()
                    ),
                }));
            }
            Ok(Some(target))
        }
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(FcError::PathIo {
            path: active_pointer.to_path_buf(),
            source,
        }),
    }
}

fn reject_parent_components(field: &'static str, path: &Path) -> Result<(), FcError> {
    if has_parent_component(path) {
        return Err(FcError::Config(ConfigError::InvalidValue {
            field,
            reason: format!("path must not contain '..': {}", path.display()),
        }));
    }
    Ok(())
}

fn reject_symlinked_install_root(install_root: &Path) -> Result<(), FcError> {
    let mut prefix = PathBuf::new();
    for component in install_root.components() {
        prefix.push(component.as_os_str());
        match fs::symlink_metadata(&prefix) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(FcError::Config(ConfigError::InvalidValue {
                    field: "install_cleanup.install_root",
                    reason: format!(
                        "install root must not pass through a symlink: {}",
                        prefix.display()
                    ),
                }));
            }
            Ok(metadata) if prefix == install_root && !metadata.is_dir() => {
                return Err(FcError::Config(ConfigError::InvalidValue {
                    field: "install_cleanup.install_root",
                    reason: format!(
                        "install root must be a directory path, got {}",
                        install_root.display()
                    ),
                }));
            }
            Ok(_) => {}
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(source) => {
                return Err(FcError::PathIo {
                    path: prefix,
                    source,
                });
            }
        }
    }
    Ok(())
}

fn has_parent_component(path: &Path) -> bool {
    path.components()
        .any(|component| matches!(component, Component::ParentDir))
}

fn lexical_normalize(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::RootDir | Component::Prefix(_) | Component::Normal(_) => {
                normalized.push(component.as_os_str());
            }
            Component::ParentDir => {}
        }
    }
    normalized
}
