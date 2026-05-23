use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use m80_firecracker::FcError;
use serde::Serialize;

const DEFAULT_PROFILE_NAME: &str = "default";
const DEFAULT_PROFILE_FILE: &str = "default.toml";
const INSTALL_PROVENANCE_FILE: &str = "install-provenance.json";

pub(in crate::cmds) struct InstalledDefaultProfile<'a> {
    pub(in crate::cmds) artifact_dir: &'a Path,
    pub(in crate::cmds) run_root: &'a Path,
    pub(in crate::cmds) profile_dir: &'a Path,
    pub(in crate::cmds) config_path: &'a Path,
    pub(in crate::cmds) binary_config: m80_preflight::BinaryDiscoveryConfig,
    pub(in crate::cmds) release_tag: Option<String>,
    pub(in crate::cmds) m80_version: String,
    pub(in crate::cmds) host_binaries_manifest: &'a Path,
    pub(in crate::cmds) adopt_existing_config: bool,
    pub(in crate::cmds) adoption_command: String,
}

#[derive(Serialize)]
struct RuntimeProfileToml {
    artifact_dir: PathBuf,
    kernel_image: PathBuf,
    rootfs_image: PathBuf,
    kernel_kind: String,
    guestd: PathBuf,
    guest_manifest: PathBuf,
    build_receipt: PathBuf,
    install_provenance: PathBuf,
    host_binaries_manifest: PathBuf,
    firecracker_bin: PathBuf,
    firecracker_seccomp_filter: PathBuf,
    jailer_bin: PathBuf,
    jailer_harden_bin: PathBuf,
    net_helper_bin: PathBuf,
    run_root: PathBuf,
    #[serde(skip_serializing_if = "Option::is_none")]
    release_tag: Option<String>,
    m80_version: String,
    description: String,
}

pub(in crate::cmds) fn write_installed_default_profile(
    input: InstalledDefaultProfile<'_>,
) -> Result<PathBuf, FcError> {
    let profile_path = input.profile_dir.join(DEFAULT_PROFILE_FILE);
    let mut transaction = InstalledProfileTransaction::capture(&profile_path, input.config_path)?;
    let result = (|| {
        let profile_contents = profile_toml(&input)?;
        let config_contents = config_toml(input.run_root)?;
        ensure_selector_write_allowed(
            "profile",
            &profile_path,
            &profile_contents,
            input.adopt_existing_config,
            &input.adoption_command,
        )?;
        ensure_selector_write_allowed(
            "config",
            input.config_path,
            &config_contents,
            input.adopt_existing_config,
            &input.adoption_command,
        )?;
        write_profile_file(&profile_path, &profile_contents)?;
        write_profile_file(input.config_path, &config_contents)?;
        Ok(())
    })();
    if let Err(err) = result {
        transaction.rollback();
        return Err(err);
    }
    Ok(profile_path)
}

fn profile_toml(input: &InstalledDefaultProfile<'_>) -> Result<String, FcError> {
    let manifest =
        m80_image_manifest::Manifest::read(&input.artifact_dir.join("output.ext4.manifest.json"))
            .map_err(FcError::Manifest)?;
    let kernel_kind = match manifest.kernel_kind {
        m80_image_manifest::KernelKind::Stock => "stock",
        m80_image_manifest::KernelKind::Stripped => "stripped",
    };
    let profile = RuntimeProfileToml {
        artifact_dir: input.artifact_dir.to_path_buf(),
        kernel_image: input.artifact_dir.join("vmlinux"),
        rootfs_image: input.artifact_dir.join("output.ext4"),
        kernel_kind: kernel_kind.to_owned(),
        guestd: input.artifact_dir.join("m80-guestd"),
        guest_manifest: input.artifact_dir.join("output.ext4.manifest.json"),
        build_receipt: input.artifact_dir.join("output.ext4.build-receipt.json"),
        install_provenance: input.artifact_dir.join(INSTALL_PROVENANCE_FILE),
        host_binaries_manifest: input.host_binaries_manifest.to_path_buf(),
        firecracker_bin: input.binary_config.firecracker_bin.clone(),
        firecracker_seccomp_filter: input.binary_config.firecracker_seccomp_filter.clone(),
        jailer_bin: input.binary_config.jailer_bin.clone(),
        jailer_harden_bin: input.binary_config.jailer_harden_bin.clone(),
        net_helper_bin: input.binary_config.net_helper_bin.clone(),
        run_root: input.run_root.to_path_buf(),
        release_tag: input.release_tag.clone(),
        m80_version: input.m80_version.clone(),
        description: "m80 installed default profile".to_owned(),
    };
    toml::to_string_pretty(&profile).map_err(|source| FcError::Json {
        context: "serialize installed default profile",
        source: serde_json::Error::io(io::Error::new(io::ErrorKind::Other, source)),
    })
}

fn write_profile_file(path: &Path, contents: &str) -> Result<(), FcError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| FcError::PathIo {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    fs::write(path, contents).map_err(|source| FcError::PathIo {
        path: path.to_path_buf(),
        source,
    })?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o644)).map_err(|source| FcError::PathIo {
        path: path.to_path_buf(),
        source,
    })
}

fn config_toml(run_root: &Path) -> Result<String, FcError> {
    let mut table = toml::map::Map::new();
    table.insert(
        "default_profile".to_owned(),
        toml::Value::String(DEFAULT_PROFILE_NAME.to_owned()),
    );
    table.insert(
        "run_root".to_owned(),
        toml::Value::String(run_root.display().to_string()),
    );
    toml::to_string_pretty(&toml::Value::Table(table)).map_err(|source| FcError::Json {
        context: "serialize installed default config",
        source: serde_json::Error::io(io::Error::new(io::ErrorKind::Other, source)),
    })
}

fn ensure_selector_write_allowed(
    kind: &str,
    path: &Path,
    proposed_contents: &str,
    adopt_existing_config: bool,
    adoption_command: &str,
) -> Result<(), FcError> {
    let existing = match fs::read_to_string(path) {
        Ok(existing) => existing,
        Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(source) => {
            return Err(FcError::PathIo {
                path: path.to_path_buf(),
                source,
            });
        }
    };
    if existing == proposed_contents || adopt_existing_config {
        return Ok(());
    }
    Err(FcError::Config(
        m80_firecracker::ConfigError::InvalidValue {
            field: "install.config_preservation",
            reason: format!(
                "existing m80 {kind} would be overwritten: old_path={} proposed_path={}; adoption command: {}; backup command: cp -a {} {}",
                path.display(),
                path.display(),
                adoption_command,
                shell_single_quote(path),
                shell_single_quote(&backup_path(path))
            ),
        },
    ))
}

fn backup_path(path: &Path) -> PathBuf {
    let mut backup = path.as_os_str().to_os_string();
    backup.push(".backup");
    PathBuf::from(backup)
}

fn shell_single_quote(path: &Path) -> String {
    format!("'{}'", path.display().to_string().replace('\'', "'\\''"))
}

pub(in crate::cmds) struct InstalledProfileTransaction {
    profile: PathBackup,
    config: PathBackup,
}

impl InstalledProfileTransaction {
    pub(in crate::cmds) fn capture(
        profile_path: &Path,
        config_path: &Path,
    ) -> Result<Self, FcError> {
        Ok(Self {
            profile: PathBackup::capture(profile_path)?,
            config: PathBackup::capture(config_path)?,
        })
    }

    pub(in crate::cmds) fn rollback(&mut self) {
        let _ = self.profile.restore();
        let _ = self.config.restore();
    }
}

struct PathBackup {
    path: PathBuf,
    state: PathState,
}

enum PathState {
    Missing,
    File {
        contents: Vec<u8>,
        permissions: fs::Permissions,
    },
    Symlink {
        target: PathBuf,
        target_state: SymlinkTargetState,
    },
    Other,
}

enum SymlinkTargetState {
    Missing,
    File {
        contents: Vec<u8>,
        permissions: fs::Permissions,
    },
    Other,
}

impl PathBackup {
    fn capture(path: &Path) -> Result<Self, FcError> {
        let state = match fs::symlink_metadata(path) {
            Ok(metadata) if metadata.is_file() => PathState::File {
                contents: fs::read(path).map_err(|source| FcError::PathIo {
                    path: path.to_path_buf(),
                    source,
                })?,
                permissions: metadata.permissions(),
            },
            Ok(metadata) if metadata.file_type().is_symlink() => {
                let target = fs::read_link(path).map_err(|source| FcError::PathIo {
                    path: path.to_path_buf(),
                    source,
                })?;
                let target = resolve_link_target(path, target);
                PathState::Symlink {
                    target_state: capture_symlink_target(&target)?,
                    target,
                }
            }
            Ok(_) => PathState::Other,
            Err(source) if source.kind() == io::ErrorKind::NotFound => PathState::Missing,
            Err(source) => {
                return Err(FcError::PathIo {
                    path: path.to_path_buf(),
                    source,
                });
            }
        };
        Ok(Self {
            path: path.to_path_buf(),
            state,
        })
    }

    fn restore(&self) -> Result<(), FcError> {
        match &self.state {
            PathState::Missing => {
                if self.path.exists() {
                    fs::remove_file(&self.path).map_err(|source| FcError::PathIo {
                        path: self.path.clone(),
                        source,
                    })?;
                }
            }
            PathState::File {
                contents,
                permissions,
            } => {
                if let Some(parent) = self.path.parent() {
                    fs::create_dir_all(parent).map_err(|source| FcError::PathIo {
                        path: parent.to_path_buf(),
                        source,
                    })?;
                }
                fs::write(&self.path, contents).map_err(|source| FcError::PathIo {
                    path: self.path.clone(),
                    source,
                })?;
                fs::set_permissions(&self.path, permissions.clone()).map_err(|source| {
                    FcError::PathIo {
                        path: self.path.clone(),
                        source,
                    }
                })?;
            }
            PathState::Symlink {
                target,
                target_state,
            } => match target_state {
                SymlinkTargetState::Missing => {
                    if target.exists() {
                        fs::remove_file(target).map_err(|source| FcError::PathIo {
                            path: target.clone(),
                            source,
                        })?;
                    }
                }
                SymlinkTargetState::File {
                    contents,
                    permissions,
                } => {
                    if let Some(parent) = target.parent() {
                        fs::create_dir_all(parent).map_err(|source| FcError::PathIo {
                            path: parent.to_path_buf(),
                            source,
                        })?;
                    }
                    fs::write(target, contents).map_err(|source| FcError::PathIo {
                        path: target.clone(),
                        source,
                    })?;
                    fs::set_permissions(target, permissions.clone()).map_err(|source| {
                        FcError::PathIo {
                            path: target.clone(),
                            source,
                        }
                    })?;
                }
                SymlinkTargetState::Other => {}
            },
            PathState::Other => {}
        }
        Ok(())
    }
}

fn capture_symlink_target(target: &Path) -> Result<SymlinkTargetState, FcError> {
    match fs::metadata(target) {
        Ok(metadata) if metadata.is_file() => Ok(SymlinkTargetState::File {
            contents: fs::read(target).map_err(|source| FcError::PathIo {
                path: target.to_path_buf(),
                source,
            })?,
            permissions: metadata.permissions(),
        }),
        Ok(_) => Ok(SymlinkTargetState::Other),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(SymlinkTargetState::Missing),
        Err(source) => Err(FcError::PathIo {
            path: target.to_path_buf(),
            source,
        }),
    }
}

fn resolve_link_target(link_path: &Path, target: PathBuf) -> PathBuf {
    if target.is_absolute() {
        return target;
    }
    link_path
        .parent()
        .map(|parent| parent.join(&target))
        .unwrap_or(target)
}
