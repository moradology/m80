use std::fs;
use std::io;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};

use m80_firecracker::{ConfigError, FcError};

use super::metadata::{write_flat_artifact_metadata, INSTALL_PROVENANCE_FILE};
use super::remove_path_if_exists;

#[derive(Debug, Clone)]
pub(super) struct FlatProjection {
    pub(super) artifact_dir: PathBuf,
    pub(super) host_binaries_manifest: PathBuf,
    pub(super) binary_config: m80_preflight::BinaryDiscoveryConfig,
    pub(super) include_jailer_harden: bool,
}

pub(super) fn publish_flat_projection(
    install_root: &Path,
    final_dir: &Path,
    versioned_binary_config: &m80_preflight::BinaryDiscoveryConfig,
    release_tag: &str,
    staging_root: &Path,
) -> Result<(FlatProjection, FlatProjectionGuard), FcError> {
    let bin_dir = install_root.join("bin");
    let artifact_dir = install_root.join("artifacts");
    let versioned_artifacts = final_dir.join("artifacts");
    let backup_root = staging_root.join("flat-backup");
    let guard = FlatProjectionGuard::capture(install_root, &backup_root)?;

    prepare_flat_dir(&bin_dir)?;
    prepare_flat_dir(&artifact_dir)?;

    let install_jailer_harden = selected_launch_path_for_flat_projection(versioned_binary_config)?
        == m80_preflight::LaunchPath::Wrapper;

    for name in ["m80", "m80-net-helper"] {
        atomic_hardlink_replace(&final_dir.join("bin").join(name), &bin_dir.join(name))?;
    }
    if install_jailer_harden {
        atomic_hardlink_replace(
            &final_dir.join("bin/m80-jailer-harden"),
            &bin_dir.join("m80-jailer-harden"),
        )?;
    } else {
        remove_path_if_exists(&bin_dir.join("m80-jailer-harden"))?;
    }
    for name in ["m80-guestd", "output.ext4", "vmlinux"] {
        atomic_hardlink_replace(&versioned_artifacts.join(name), &artifact_dir.join(name))?;
    }

    let metadata_stage = staging_root.join("flat-metadata").join("artifacts");
    write_flat_artifact_metadata(
        &versioned_artifacts,
        &metadata_stage,
        &artifact_dir,
        release_tag,
    )?;
    for name in [
        "output.ext4.manifest.json",
        "output.ext4.build-receipt.json",
        INSTALL_PROVENANCE_FILE,
    ] {
        atomic_copy_replace(&metadata_stage.join(name), &artifact_dir.join(name), 0o644)?;
    }
    publish_flat_proof_cache(&versioned_artifacts, &artifact_dir)?;

    let flat_binary_config = m80_preflight::BinaryDiscoveryConfig {
        firecracker_bin: versioned_binary_config.firecracker_bin.clone(),
        firecracker_seccomp_filter: versioned_binary_config.firecracker_seccomp_filter.clone(),
        jailer_bin: versioned_binary_config.jailer_bin.clone(),
        jailer_harden_bin: bin_dir.join("m80-jailer-harden"),
        net_helper_bin: bin_dir.join("m80-net-helper"),
        expected_firecracker_version: versioned_binary_config.expected_firecracker_version.clone(),
    };
    let host_binaries_manifest = write_flat_host_binaries_manifest(
        &artifact_dir,
        &bin_dir,
        &flat_binary_config,
        install_jailer_harden,
    )?;
    Ok((
        FlatProjection {
            artifact_dir,
            host_binaries_manifest,
            binary_config: flat_binary_config,
            include_jailer_harden: install_jailer_harden,
        },
        guard,
    ))
}

fn selected_launch_path_for_flat_projection(
    binary_config: &m80_preflight::BinaryDiscoveryConfig,
) -> Result<m80_preflight::LaunchPath, FcError> {
    #[cfg(debug_assertions)]
    if std::env::var_os("M80_INSTALL_FORCE_SYSTEMD_FLAT_PROJECTION").is_some() {
        return Ok(m80_preflight::LaunchPath::Systemd);
    }

    m80_preflight::select_host_launch_path(binary_config).map_err(FcError::Preflight)
}

fn write_flat_host_binaries_manifest(
    artifact_dir: &Path,
    bin_dir: &Path,
    binary_config: &m80_preflight::BinaryDiscoveryConfig,
    include_jailer_harden: bool,
) -> Result<PathBuf, FcError> {
    let path = artifact_dir.join("host-binaries.manifest.json");
    let tmp = sibling_tmp_path(&path, "host-binaries.manifest.json");
    remove_path_if_exists(&tmp)?;
    let config = m80_preflight::HostBinariesManifestConfig {
        firecracker_bin: binary_config.firecracker_bin.clone(),
        firecracker_seccomp_filter: binary_config.firecracker_seccomp_filter.clone(),
        jailer_bin: binary_config.jailer_bin.clone(),
        jailer_harden_bin: binary_config.jailer_harden_bin.clone(),
        include_jailer_harden,
        net_helper_bin: binary_config.net_helper_bin.clone(),
        m80_bin: bin_dir.join("m80"),
        expected_firecracker_version: binary_config.expected_firecracker_version.clone(),
    };
    m80_preflight::write_host_binaries_manifest(&config, &tmp)?;
    fs::set_permissions(&tmp, fs::Permissions::from_mode(0o644)).map_err(|source| {
        FcError::PathIo {
            path: tmp.clone(),
            source,
        }
    })?;
    rename_tmp_into_place(&tmp, &path)?;
    Ok(path)
}

fn publish_flat_proof_cache(
    versioned_artifacts: &Path,
    artifact_dir: &Path,
) -> Result<(), FcError> {
    let source = versioned_artifacts.join("release-proof-cache");
    match fs::symlink_metadata(&source) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
            hardlink_copy_tree(&source, &artifact_dir.join("release-proof-cache"))
        }
        Ok(_) => Err(FcError::Config(ConfigError::InvalidValue {
            field: "install.release_proof_cache",
            reason: format!(
                "release-proof-cache must be a directory: {}",
                source.display()
            ),
        })),
        Err(source_err) if source_err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source_err) => Err(FcError::PathIo {
            path: source,
            source: source_err,
        }),
    }
}

fn prepare_flat_dir(path: &Path) -> Result<(), FcError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(FcError::Config(ConfigError::InvalidValue {
                field: "install.flat_projection",
                reason: format!(
                    "flat projection directory must not be a symlink: {}",
                    path.display()
                ),
            }));
        }
        Ok(metadata) if metadata.is_dir() => {}
        Ok(_) => {
            return Err(FcError::Config(ConfigError::InvalidValue {
                field: "install.flat_projection",
                reason: format!(
                    "flat projection path already exists but is not a directory: {}",
                    path.display()
                ),
            }));
        }
        Err(source) if source.kind() == io::ErrorKind::NotFound => {
            fs::create_dir_all(path).map_err(|source| FcError::PathIo {
                path: path.to_path_buf(),
                source,
            })?;
        }
        Err(source) => {
            return Err(FcError::PathIo {
                path: path.to_path_buf(),
                source,
            });
        }
    }
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).map_err(|source| FcError::PathIo {
        path: path.to_path_buf(),
        source,
    })
}

fn hardlink_copy_tree(source: &Path, destination: &Path) -> Result<(), FcError> {
    let parent = destination_parent(destination)?;
    fs::create_dir_all(parent).map_err(|source_err| FcError::PathIo {
        path: parent.to_path_buf(),
        source: source_err,
    })?;
    let tmp = sibling_tmp_path(destination, "tree");
    remove_path_if_exists(&tmp)?;
    copy_tree_contents(source, &tmp)?;
    if let Err(err) = replace_path_with_prepared_tree(&tmp, destination) {
        let _ = remove_path_if_exists(&tmp);
        return Err(err);
    }
    Ok(())
}

fn copy_tree_contents(source: &Path, destination: &Path) -> Result<(), FcError> {
    fs::create_dir(destination).map_err(|source_err| FcError::PathIo {
        path: destination.to_path_buf(),
        source: source_err,
    })?;
    fs::set_permissions(destination, fs::Permissions::from_mode(0o755)).map_err(|source_err| {
        FcError::PathIo {
            path: destination.to_path_buf(),
            source: source_err,
        }
    })?;
    for entry in fs::read_dir(source).map_err(|source_err| FcError::PathIo {
        path: source.to_path_buf(),
        source: source_err,
    })? {
        let entry = entry.map_err(|source_err| FcError::PathIo {
            path: source.to_path_buf(),
            source: source_err,
        })?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let metadata =
            fs::symlink_metadata(&source_path).map_err(|source_err| FcError::PathIo {
                path: source_path.clone(),
                source: source_err,
            })?;
        if metadata.file_type().is_symlink() {
            return Err(FcError::Config(ConfigError::InvalidValue {
                field: "install.release_proof_cache",
                reason: format!(
                    "release-proof-cache must not contain symlinks: {}",
                    source_path.display()
                ),
            }));
        }
        if metadata.is_dir() {
            copy_tree_contents(&source_path, &destination_path)?;
        } else if metadata.is_file() {
            fs::hard_link(&source_path, &destination_path).map_err(|source_err| {
                FcError::PathIo {
                    path: destination_path,
                    source: source_err,
                }
            })?;
        } else {
            return Err(FcError::Config(ConfigError::InvalidValue {
                field: "install.release_proof_cache",
                reason: format!(
                    "release-proof-cache entries must be files or directories: {}",
                    source_path.display()
                ),
            }));
        }
    }
    Ok(())
}

fn replace_path_with_prepared_tree(tmp: &Path, destination: &Path) -> Result<(), FcError> {
    let old = sibling_tmp_path(destination, "old-tree");
    remove_path_if_exists(&old)?;
    match fs::symlink_metadata(destination) {
        Ok(_) => {
            fs::rename(destination, &old).map_err(|source| FcError::PathIo {
                path: old.clone(),
                source,
            })?;
            if let Err(err) = rename_tmp_into_place(tmp, destination) {
                let _ = fs::rename(&old, destination);
                return Err(err);
            }
            remove_path_if_exists(&old)
        }
        Err(source) if source.kind() == io::ErrorKind::NotFound => {
            rename_tmp_into_place(tmp, destination)
        }
        Err(source) => Err(FcError::PathIo {
            path: destination.to_path_buf(),
            source,
        }),
    }
}

fn atomic_hardlink_replace(source: &Path, destination: &Path) -> Result<(), FcError> {
    reject_directory_destination(destination)?;
    let parent = destination_parent(destination)?;
    fs::create_dir_all(parent).map_err(|source_err| FcError::PathIo {
        path: parent.to_path_buf(),
        source: source_err,
    })?;
    let tmp = sibling_tmp_path(destination, "hardlink");
    remove_path_if_exists(&tmp)?;
    fs::hard_link(source, &tmp).map_err(|source_err| FcError::PathIo {
        path: tmp.clone(),
        source: source_err,
    })?;
    if let Err(err) = rename_tmp_into_place(&tmp, destination) {
        let _ = remove_path_if_exists(&tmp);
        return Err(err);
    }
    Ok(())
}

fn atomic_copy_replace(source: &Path, destination: &Path, mode: u32) -> Result<(), FcError> {
    reject_directory_destination(destination)?;
    let parent = destination_parent(destination)?;
    fs::create_dir_all(parent).map_err(|source_err| FcError::PathIo {
        path: parent.to_path_buf(),
        source: source_err,
    })?;
    let tmp = sibling_tmp_path(destination, "copy");
    remove_path_if_exists(&tmp)?;
    fs::copy(source, &tmp).map_err(|source_err| FcError::PathIo {
        path: tmp.clone(),
        source: source_err,
    })?;
    fs::set_permissions(&tmp, fs::Permissions::from_mode(mode)).map_err(|source_err| {
        FcError::PathIo {
            path: tmp.clone(),
            source: source_err,
        }
    })?;
    if let Err(err) = rename_tmp_into_place(&tmp, destination) {
        let _ = remove_path_if_exists(&tmp);
        return Err(err);
    }
    Ok(())
}

fn reject_directory_destination(destination: &Path) -> Result<(), FcError> {
    match fs::symlink_metadata(destination) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
            Err(FcError::Config(ConfigError::InvalidValue {
                field: "install.flat_projection",
                reason: format!(
                    "flat projection target already exists as directory: {}",
                    destination.display()
                ),
            }))
        }
        Ok(_) => Ok(()),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(FcError::PathIo {
            path: destination.to_path_buf(),
            source,
        }),
    }
}

fn destination_parent(destination: &Path) -> Result<&Path, FcError> {
    destination.parent().ok_or_else(|| {
        FcError::Config(ConfigError::InvalidValue {
            field: "install.flat_projection",
            reason: format!(
                "flat projection path has no parent: {}",
                destination.display()
            ),
        })
    })
}

fn sibling_tmp_path(destination: &Path, label: &str) -> PathBuf {
    let file_name = destination
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("entry");
    destination.with_file_name(format!(".{file_name}.{label}.tmp-{}", std::process::id()))
}

fn rename_tmp_into_place(tmp: &Path, destination: &Path) -> Result<(), FcError> {
    fs::rename(tmp, destination).map_err(|source| FcError::PathIo {
        path: destination.to_path_buf(),
        source,
    })
}

pub(super) struct FlatProjectionGuard {
    entries: Vec<FlatPathBackup>,
    created_dirs: Vec<PathBuf>,
    armed: bool,
}

struct FlatPathBackup {
    path: PathBuf,
    state: FlatPathState,
}

enum FlatPathState {
    Missing,
    File { backup: PathBuf },
    Directory { backup: PathBuf },
    Symlink { target: PathBuf },
}

impl FlatProjectionGuard {
    fn capture(install_root: &Path, backup_root: &Path) -> Result<Self, FcError> {
        remove_path_if_exists(backup_root)?;
        fs::create_dir_all(backup_root).map_err(|source| FcError::PathIo {
            path: backup_root.to_path_buf(),
            source,
        })?;
        let created_dirs = [install_root.join("bin"), install_root.join("artifacts")]
            .into_iter()
            .filter(|path| {
                fs::symlink_metadata(path)
                    .is_err_and(|source| source.kind() == io::ErrorKind::NotFound)
            })
            .collect();
        let mut entries = Vec::new();
        for (index, path) in flat_managed_paths(install_root).into_iter().enumerate() {
            entries.push(FlatPathBackup::capture(
                path,
                &backup_root.join(index.to_string()),
            )?);
        }
        Ok(Self {
            entries,
            created_dirs,
            armed: true,
        })
    }

    pub(super) fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for FlatProjectionGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        for entry in self.entries.iter().rev() {
            let _ = entry.restore();
        }
        for path in self.created_dirs.iter().rev() {
            let _ = fs::remove_dir(path);
        }
    }
}

impl FlatPathBackup {
    fn capture(path: PathBuf, backup: &Path) -> Result<Self, FcError> {
        let state = match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_symlink() => FlatPathState::Symlink {
                target: fs::read_link(&path).map_err(|source| FcError::PathIo {
                    path: path.clone(),
                    source,
                })?,
            },
            Ok(metadata) if metadata.is_file() => {
                if let Some(parent) = backup.parent() {
                    fs::create_dir_all(parent).map_err(|source| FcError::PathIo {
                        path: parent.to_path_buf(),
                        source,
                    })?;
                }
                fs::hard_link(&path, backup).map_err(|source| FcError::PathIo {
                    path: backup.to_path_buf(),
                    source,
                })?;
                FlatPathState::File {
                    backup: backup.to_path_buf(),
                }
            }
            Ok(metadata) if metadata.is_dir() => {
                backup_directory_tree(&path, backup)?;
                FlatPathState::Directory {
                    backup: backup.to_path_buf(),
                }
            }
            Ok(_) => {
                return Err(FcError::Config(ConfigError::InvalidValue {
                    field: "install.flat_projection",
                    reason: format!(
                        "flat projection target has unsupported file type: {}",
                        path.display()
                    ),
                }));
            }
            Err(source) if source.kind() == io::ErrorKind::NotFound => FlatPathState::Missing,
            Err(source) => return Err(FcError::PathIo { path, source }),
        };
        Ok(Self { path, state })
    }

    fn restore(&self) -> Result<(), FcError> {
        remove_path_if_exists(&self.path)?;
        match &self.state {
            FlatPathState::Missing => Ok(()),
            FlatPathState::File { backup } | FlatPathState::Directory { backup } => {
                if let Some(parent) = self.path.parent() {
                    fs::create_dir_all(parent).map_err(|source| FcError::PathIo {
                        path: parent.to_path_buf(),
                        source,
                    })?;
                }
                fs::rename(backup, &self.path).map_err(|source| FcError::PathIo {
                    path: self.path.clone(),
                    source,
                })
            }
            FlatPathState::Symlink { target } => {
                let tmp = sibling_tmp_path(&self.path, "rollback-symlink");
                remove_path_if_exists(&tmp)?;
                symlink(target, &tmp).map_err(|source| FcError::PathIo {
                    path: tmp.clone(),
                    source,
                })?;
                rename_tmp_into_place(&tmp, &self.path)
            }
        }
    }
}

fn backup_directory_tree(source: &Path, destination: &Path) -> Result<(), FcError> {
    let parent = destination_parent(destination)?;
    fs::create_dir_all(parent).map_err(|source_err| FcError::PathIo {
        path: parent.to_path_buf(),
        source: source_err,
    })?;
    let tmp = sibling_tmp_path(destination, "backup-tree");
    remove_path_if_exists(&tmp)?;
    copy_backup_tree_contents(source, &tmp)?;
    if let Err(err) = replace_path_with_prepared_tree(&tmp, destination) {
        let _ = remove_path_if_exists(&tmp);
        return Err(err);
    }
    Ok(())
}

fn copy_backup_tree_contents(source: &Path, destination: &Path) -> Result<(), FcError> {
    fs::create_dir(destination).map_err(|source_err| FcError::PathIo {
        path: destination.to_path_buf(),
        source: source_err,
    })?;
    fs::set_permissions(destination, fs::Permissions::from_mode(0o755)).map_err(|source_err| {
        FcError::PathIo {
            path: destination.to_path_buf(),
            source: source_err,
        }
    })?;
    for entry in fs::read_dir(source).map_err(|source_err| FcError::PathIo {
        path: source.to_path_buf(),
        source: source_err,
    })? {
        let entry = entry.map_err(|source_err| FcError::PathIo {
            path: source.to_path_buf(),
            source: source_err,
        })?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let metadata =
            fs::symlink_metadata(&source_path).map_err(|source_err| FcError::PathIo {
                path: source_path.clone(),
                source: source_err,
            })?;
        if metadata.file_type().is_symlink() {
            let target = fs::read_link(&source_path).map_err(|source_err| FcError::PathIo {
                path: source_path.clone(),
                source: source_err,
            })?;
            symlink(&target, &destination_path).map_err(|source_err| FcError::PathIo {
                path: destination_path,
                source: source_err,
            })?;
        } else if metadata.is_dir() {
            copy_backup_tree_contents(&source_path, &destination_path)?;
        } else if metadata.is_file() {
            fs::hard_link(&source_path, &destination_path).map_err(|source_err| {
                FcError::PathIo {
                    path: destination_path,
                    source: source_err,
                }
            })?;
        } else {
            return Err(FcError::Config(ConfigError::InvalidValue {
                field: "install.flat_projection",
                reason: format!(
                    "flat projection backup contains unsupported file type: {}",
                    source_path.display()
                ),
            }));
        }
    }
    Ok(())
}

fn flat_managed_paths(install_root: &Path) -> Vec<PathBuf> {
    let bin_dir = install_root.join("bin");
    let artifact_dir = install_root.join("artifacts");
    vec![
        bin_dir.join("m80"),
        bin_dir.join("m80-jailer-harden"),
        bin_dir.join("m80-net-helper"),
        artifact_dir.join("m80-guestd"),
        artifact_dir.join("output.ext4"),
        artifact_dir.join("vmlinux"),
        artifact_dir.join("output.ext4.manifest.json"),
        artifact_dir.join("output.ext4.build-receipt.json"),
        artifact_dir.join(INSTALL_PROVENANCE_FILE),
        artifact_dir.join("host-binaries.manifest.json"),
        artifact_dir.join("release-proof-cache"),
    ]
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::MetadataExt;

    use super::{backup_directory_tree, hardlink_copy_tree};

    #[test]
    fn hardlink_copy_tree_replaces_stale_destination_entries() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        let destination = temp.path().join("destination");
        fs::create_dir_all(source.join("nested")).unwrap();
        fs::write(source.join("manifest.json"), b"manifest").unwrap();
        fs::write(source.join("nested/proof.json"), b"proof").unwrap();
        fs::create_dir_all(destination.join("nested")).unwrap();
        fs::write(destination.join("stale.json"), b"stale").unwrap();
        fs::write(destination.join("nested/stale.json"), b"nested stale").unwrap();

        hardlink_copy_tree(&source, &destination).unwrap();

        assert!(destination.join("manifest.json").is_file());
        assert!(destination.join("nested/proof.json").is_file());
        assert!(!destination.join("stale.json").exists());
        assert!(!destination.join("nested/stale.json").exists());
        assert_same_inode(
            &source.join("manifest.json"),
            &destination.join("manifest.json"),
        );
        assert_same_inode(
            &source.join("nested/proof.json"),
            &destination.join("nested/proof.json"),
        );
    }

    #[test]
    fn backup_directory_tree_preserves_legacy_symlinks_without_following() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        let destination = temp.path().join("backup");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("manifest.json"), b"manifest").unwrap();
        std::os::unix::fs::symlink(
            "../versions/v0.2.20/artifacts/release-proof-cache",
            source.join("release-proof-cache"),
        )
        .unwrap();

        backup_directory_tree(&source, &destination).unwrap();

        assert_same_inode(
            &source.join("manifest.json"),
            &destination.join("manifest.json"),
        );
        assert_eq!(
            fs::read_link(destination.join("release-proof-cache")).unwrap(),
            std::path::PathBuf::from("../versions/v0.2.20/artifacts/release-proof-cache")
        );
    }

    fn assert_same_inode(left: &std::path::Path, right: &std::path::Path) {
        let left = fs::metadata(left).unwrap();
        let right = fs::metadata(right).unwrap();
        assert_eq!((left.dev(), left.ino()), (right.dev(), right.ino()));
    }
}
