//! Firecracker and jailer binary discovery.

use std::env;
use std::fs::File;
use std::io::{Read as _, Seek as _, SeekFrom};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::Duration;

use crate::cve_floor::verify_firecracker_cve_floor;
use crate::firecracker_train::{
    enforce_configured_firecracker_version, enforce_jailer_pairing,
    parse_firecracker_version_output, parse_jailer_version_output, FirecrackerTrainPolicy,
};
use crate::{LaunchPath, PreflightError};
use m80_image_manifest::{
    ConditionalHostBinaryEntry, HostBinariesManifest, HostBinaryAbsentWhen, HostBinaryEntry,
    HostBinaryName, HostLaunchMaterialEntry, HostLaunchMaterialName,
};
use nix::libc::O_NOFOLLOW;
use sha2::{Digest, Sha256};

/// Environment key for overriding the Firecracker binary path.
pub const ENV_FIRECRACKER_BIN: &str = "M80_FIRECRACKER_BIN";
/// Environment key for enabling an exact Firecracker version check.
pub const ENV_FIRECRACKER_VERSION: &str = "M80_FIRECRACKER_VERSION";
/// Environment key for overriding the Firecracker advanced seccomp filter path.
pub const ENV_FIRECRACKER_SECCOMP_FILTER: &str = "M80_FIRECRACKER_SECCOMP_FILTER";
/// Environment key for overriding the jailer binary path.
pub(crate) const ENV_JAILER_BIN: &str = "M80_JAILER_BIN";
/// Environment key for overriding the m80 jailer hardening wrapper path.
pub(crate) const ENV_JAILER_HARDEN_BIN: &str = "M80_JAILER_HARDEN_BIN";
/// Environment key for overriding the m80 network helper path.
pub(crate) const ENV_NET_HELPER_BIN: &str = "M80_NET_HELPER_BIN";

/// Default Firecracker binary location when no env override is present.
pub const DEFAULT_FIRECRACKER_BIN: &str = "/opt/firecracker/bin/firecracker";
/// Default Firecracker advanced seccomp filter location.
pub const DEFAULT_FIRECRACKER_SECCOMP_FILTER: &str =
    "/opt/firecracker/bin/firecracker-seccomp-filter.bin";
/// Default jailer binary location when no env override is present.
pub(crate) const DEFAULT_JAILER_BIN: &str = "/opt/firecracker/bin/jailer";
/// Default m80 jailer hardening wrapper location when no env override is present.
pub(crate) const DEFAULT_JAILER_HARDEN_BIN: &str = "/opt/m80/bin/m80-jailer-harden";
/// Default m80 network helper location when no env override is present.
pub(crate) const DEFAULT_NET_HELPER_BIN: &str = "/opt/m80/bin/m80-net-helper";
/// Default installed m80 CLI location.
pub const DEFAULT_M80_BIN: &str = "/opt/m80/bin/m80";

/// Inputs for the binary discovery preflight step.
#[derive(Debug, Clone)]
pub struct BinaryDiscoveryConfig {
    /// Firecracker binary path to probe with `--version`.
    pub firecracker_bin: PathBuf,
    /// Firecracker advanced seccomp filter path.
    pub firecracker_seccomp_filter: PathBuf,
    /// Jailer binary path to require on disk.
    pub jailer_bin: PathBuf,
    /// m80 jailer hardening wrapper path to require on disk.
    pub jailer_harden_bin: PathBuf,
    /// m80 network helper path to require on disk.
    pub net_helper_bin: PathBuf,
    /// Optional exact Firecracker version pin.
    pub expected_firecracker_version: Option<String>,
}

/// Inputs for generating the installed host-binaries manifest.
#[derive(Debug, Clone)]
pub struct HostBinariesManifestConfig {
    /// Firecracker binary path to probe with `--version`.
    pub firecracker_bin: PathBuf,
    /// Firecracker advanced seccomp filter path.
    pub firecracker_seccomp_filter: PathBuf,
    /// Jailer binary path to probe with `--version`.
    pub jailer_bin: PathBuf,
    /// m80 jailer hardening wrapper path.
    pub jailer_harden_bin: PathBuf,
    /// Whether the generated manifest should require and record the wrapper.
    pub include_jailer_harden: bool,
    /// m80 network helper path.
    pub net_helper_bin: PathBuf,
    /// Installed m80 CLI executable path.
    pub m80_bin: PathBuf,
    /// Optional exact Firecracker version pin.
    pub expected_firecracker_version: Option<String>,
}

impl BinaryDiscoveryConfig {
    /// Build a config from the exact m80 environment keys.
    pub fn from_env() -> Self {
        Self {
            firecracker_bin: env::var_os(ENV_FIRECRACKER_BIN)
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(DEFAULT_FIRECRACKER_BIN)),
            firecracker_seccomp_filter: env::var_os(ENV_FIRECRACKER_SECCOMP_FILTER)
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(DEFAULT_FIRECRACKER_SECCOMP_FILTER)),
            jailer_bin: env::var_os(ENV_JAILER_BIN)
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(DEFAULT_JAILER_BIN)),
            jailer_harden_bin: env::var_os(ENV_JAILER_HARDEN_BIN)
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(DEFAULT_JAILER_HARDEN_BIN)),
            net_helper_bin: env::var_os(ENV_NET_HELPER_BIN)
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(DEFAULT_NET_HELPER_BIN)),
            expected_firecracker_version: env::var(ENV_FIRECRACKER_VERSION).ok(),
        }
    }
}

impl HostBinariesManifestConfig {
    /// Build a manifest-generation config from the exact m80 environment keys.
    ///
    /// `M80_FIRECRACKER_*`, `M80_JAILER_*`, and `M80_NET_HELPER_BIN` follow
    /// [`BinaryDiscoveryConfig::from_env`]. The installed `m80` binary defaults
    /// to `/opt/m80/bin/m80`.
    #[must_use]
    pub fn from_env() -> Self {
        let binary = BinaryDiscoveryConfig::from_env();
        Self {
            firecracker_bin: binary.firecracker_bin,
            firecracker_seccomp_filter: binary.firecracker_seccomp_filter,
            jailer_bin: binary.jailer_bin,
            jailer_harden_bin: binary.jailer_harden_bin,
            include_jailer_harden: true,
            net_helper_bin: binary.net_helper_bin,
            m80_bin: PathBuf::from(DEFAULT_M80_BIN),
            expected_firecracker_version: binary.expected_firecracker_version,
        }
    }

    fn binary_discovery_config(&self) -> BinaryDiscoveryConfig {
        BinaryDiscoveryConfig {
            firecracker_bin: self.firecracker_bin.clone(),
            firecracker_seccomp_filter: self.firecracker_seccomp_filter.clone(),
            jailer_bin: self.jailer_bin.clone(),
            jailer_harden_bin: self.jailer_harden_bin.clone(),
            net_helper_bin: self.net_helper_bin.clone(),
            expected_firecracker_version: self.expected_firecracker_version.clone(),
        }
    }
}

/// Resolved binary paths and the probed Firecracker version.
#[derive(Debug, Clone)]
pub(crate) struct BinaryDiscovery {
    /// Resolved Firecracker binary path.
    pub(crate) firecracker_bin: PathBuf,
    /// Resolved Firecracker advanced seccomp filter path.
    pub(crate) firecracker_seccomp_filter: PathBuf,
    /// Version parsed from `firecracker --version`.
    pub(crate) firecracker_version: String,
    /// Resolved jailer binary path.
    pub(crate) jailer_bin: PathBuf,
    /// Version parsed from `jailer --version`.
    pub(crate) jailer_version: String,
    /// Resolved m80 jailer hardening wrapper path.
    pub(crate) jailer_harden_bin: PathBuf,
    /// Resolved m80 network helper path.
    pub(crate) net_helper_bin: PathBuf,
}

/// Resolve Firecracker and jailer binaries and fail closed on version mismatch.
pub(crate) fn discover_binaries(
    config: &BinaryDiscoveryConfig,
    cached_firecracker_version: Option<&str>,
    cached_jailer_version: Option<&str>,
    launch_path: LaunchPath,
) -> Result<BinaryDiscovery, PreflightError> {
    require_absolute_binary("firecracker", &config.firecracker_bin)?;
    require_absolute_binary(
        "firecracker seccomp filter",
        &config.firecracker_seccomp_filter,
    )?;
    require_absolute_binary("jailer", &config.jailer_bin)?;
    if launch_path == LaunchPath::Wrapper {
        require_absolute_binary("m80-jailer-harden", &config.jailer_harden_bin)?;
    }
    require_absolute_binary("m80-net-helper", &config.net_helper_bin)?;

    if !config.firecracker_bin.exists() {
        return Err(PreflightError::FirecrackerBinaryNotFound {
            path: config.firecracker_bin.clone(),
        });
    }
    verify_seccomp_filter_path(&config.firecracker_seccomp_filter)?;

    let actual_version = match cached_firecracker_version {
        Some(version) => version.to_owned(),
        None => firecracker_version(&config.firecracker_bin)?,
    };
    verify_firecracker_cve_floor(&actual_version)?;
    let train_policy = FirecrackerTrainPolicy::from_expected_firecracker_version(
        config.expected_firecracker_version.clone(),
    );
    enforce_configured_firecracker_version(&train_policy, &actual_version)?;

    if !config.jailer_bin.exists() {
        return Err(PreflightError::JailerBinaryNotFound {
            path: config.jailer_bin.clone(),
        });
    }
    let jailer_version = match cached_jailer_version {
        Some(version) => version.to_owned(),
        None => jailer_version(&config.jailer_bin)?,
    };
    enforce_jailer_pairing(&actual_version, &jailer_version)?;

    if launch_path == LaunchPath::Wrapper && !config.jailer_harden_bin.exists() {
        return Err(PreflightError::JailerHardenBinaryNotFound {
            path: config.jailer_harden_bin.clone(),
        });
    }
    if !config.net_helper_bin.exists() {
        return Err(PreflightError::NetHelperBinaryNotFound {
            path: config.net_helper_bin.clone(),
        });
    }

    Ok(BinaryDiscovery {
        firecracker_bin: config.firecracker_bin.clone(),
        firecracker_seccomp_filter: config.firecracker_seccomp_filter.clone(),
        firecracker_version: actual_version,
        jailer_bin: config.jailer_bin.clone(),
        jailer_version,
        jailer_harden_bin: config.jailer_harden_bin.clone(),
        net_helper_bin: config.net_helper_bin.clone(),
    })
}

/// Generate the installed host-binaries manifest from final host paths.
///
/// The generated manifest records the bytes at the supplied paths after
/// Firecracker/jailer discovery has accepted their release train. It is an
/// install-time artifact: release bundles must not carry it precomputed.
pub fn generate_host_binaries_manifest(
    config: &HostBinariesManifestConfig,
) -> Result<HostBinariesManifest, PreflightError> {
    require_absolute_binary("m80", &config.m80_bin)?;
    let launch_path = if config.include_jailer_harden {
        LaunchPath::Wrapper
    } else {
        LaunchPath::Systemd
    };
    let discovery = discover_binaries(&config.binary_discovery_config(), None, None, launch_path)?;

    let mut binaries = vec![
        record_host_binary(
            HostBinaryName::Firecracker,
            &discovery.firecracker_bin,
            discovery.firecracker_version.clone(),
        )?,
        record_host_binary(
            HostBinaryName::Jailer,
            &discovery.jailer_bin,
            discovery.jailer_version.clone(),
        )?,
        record_host_binary(
            HostBinaryName::M80,
            &config.m80_bin,
            host_binary_version(HostBinaryName::M80, &config.m80_bin)?,
        )?,
    ];
    let mut conditional_binaries = Vec::new();
    if config.include_jailer_harden {
        binaries.push(record_host_binary(
            HostBinaryName::M80JailerHarden,
            &discovery.jailer_harden_bin,
            host_binary_version(
                HostBinaryName::M80JailerHarden,
                &discovery.jailer_harden_bin,
            )?,
        )?);
    } else {
        conditional_binaries.push(ConditionalHostBinaryEntry {
            name: HostBinaryName::M80JailerHarden,
            absent_when: HostBinaryAbsentWhen::SystemdPathChosen,
        });
    }
    binaries.push(record_host_binary(
        HostBinaryName::M80NetHelper,
        &discovery.net_helper_bin,
        host_binary_version(HostBinaryName::M80NetHelper, &discovery.net_helper_bin)?,
    )?);

    Ok(HostBinariesManifest::new_with_conditional_binaries(
        binaries,
        vec![record_host_launch_material(
            HostLaunchMaterialName::FirecrackerSeccompFilter,
            &discovery.firecracker_seccomp_filter,
            discovery.firecracker_version,
        )?],
        conditional_binaries,
    ))
}

/// Generate and write the installed host-binaries manifest.
pub fn write_host_binaries_manifest(
    config: &HostBinariesManifestConfig,
    path: &Path,
) -> Result<(), PreflightError> {
    generate_host_binaries_manifest(config)?
        .write(path)
        .map_err(PreflightError::HostBinaryManifest)
}

fn verify_seccomp_filter_path(path: &Path) -> Result<(), PreflightError> {
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(O_NOFOLLOW)
        .open(path)
        .map_err(|source| {
            if source.kind() == std::io::ErrorKind::NotFound {
                PreflightError::FirecrackerSeccompFilterNotFound {
                    path: path.to_path_buf(),
                }
            } else {
                PreflightError::PathIo {
                    path: path.to_path_buf(),
                    source,
                }
            }
        })?;
    let metadata = file.metadata().map_err(|source| PreflightError::PathIo {
        path: path.to_path_buf(),
        source,
    })?;
    if !metadata.is_file() {
        return Err(PreflightError::FirecrackerSeccompFilterNotFound {
            path: path.to_path_buf(),
        });
    }
    let mut buf = [0u8; 1];
    let n = file
        .read(&mut buf)
        .map_err(|source| PreflightError::PathIo {
            path: path.to_path_buf(),
            source,
        })?;
    if n == 0 {
        return Err(PreflightError::FirecrackerSeccompFilterEmpty {
            path: path.to_path_buf(),
        });
    }
    Ok(())
}

pub(crate) fn verify_host_binaries(
    config: &BinaryDiscoveryConfig,
    discovery: &BinaryDiscovery,
    manifest_path: &Path,
    launch_path: LaunchPath,
) -> Result<(), PreflightError> {
    let manifest =
        HostBinariesManifest::read(manifest_path).map_err(PreflightError::HostBinaryManifest)?;
    for (name, configured_path, expected_version) in [
        (
            HostBinaryName::Firecracker,
            Some(config.firecracker_bin.as_path()),
            Some(discovery.firecracker_version.as_str()),
        ),
        (
            HostBinaryName::Jailer,
            Some(config.jailer_bin.as_path()),
            Some(discovery.jailer_version.as_str()),
        ),
        (
            HostBinaryName::M80NetHelper,
            Some(config.net_helper_bin.as_path()),
            None,
        ),
        (HostBinaryName::M80, None, None),
    ] {
        let entry = one_host_binary_entry(&manifest, name)?;
        if let Some(expected_path) = configured_path {
            if entry.path != expected_path {
                return Err(PreflightError::HostBinaryPathMismatch {
                    name: name.as_str(),
                    expected: expected_path.to_path_buf(),
                    actual: entry.path.clone(),
                });
            }
        }
        verify_host_binary_entry(entry)?;
        verify_host_binary_version(entry, expected_version)?;
    }
    match optional_host_binary_entry(&manifest, HostBinaryName::M80JailerHarden)? {
        Some(entry) => {
            if entry.path != config.jailer_harden_bin {
                return Err(PreflightError::HostBinaryPathMismatch {
                    name: HostBinaryName::M80JailerHarden.as_str(),
                    expected: config.jailer_harden_bin.clone(),
                    actual: entry.path.clone(),
                });
            }
            verify_host_binary_entry(entry)?;
            verify_host_binary_version(entry, None)?;
        }
        None if launch_path == LaunchPath::Systemd
            && manifest_allows_absent_binary(
                &manifest,
                HostBinaryName::M80JailerHarden,
                HostBinaryAbsentWhen::SystemdPathChosen,
            )? => {}
        None => {
            return Err(PreflightError::HostBinaryMissing {
                name: HostBinaryName::M80JailerHarden.as_str(),
            });
        }
    }
    let seccomp_filter = one_host_launch_material_entry(
        &manifest,
        HostLaunchMaterialName::FirecrackerSeccompFilter,
    )?;
    if seccomp_filter.path != config.firecracker_seccomp_filter {
        return Err(PreflightError::HostLaunchMaterialPathMismatch {
            name: HostLaunchMaterialName::FirecrackerSeccompFilter.as_str(),
            expected: config.firecracker_seccomp_filter.clone(),
            actual: seccomp_filter.path.clone(),
        });
    }
    verify_host_launch_material_entry(seccomp_filter)?;
    verify_host_launch_material_version(seccomp_filter, &discovery.firecracker_version)?;
    Ok(())
}

fn one_host_binary_entry(
    manifest: &HostBinariesManifest,
    name: HostBinaryName,
) -> Result<&HostBinaryEntry, PreflightError> {
    let Some(entry) = optional_host_binary_entry(manifest, name)? else {
        return Err(PreflightError::HostBinaryMissing {
            name: name.as_str(),
        });
    };
    Ok(entry)
}

fn optional_host_binary_entry(
    manifest: &HostBinariesManifest,
    name: HostBinaryName,
) -> Result<Option<&HostBinaryEntry>, PreflightError> {
    let mut matches = manifest.binaries.iter().filter(|entry| entry.name == name);
    let Some(entry) = matches.next() else {
        return Ok(None);
    };
    if matches.next().is_some() {
        return Err(PreflightError::HostBinaryDuplicate {
            name: name.as_str(),
        });
    }
    Ok(Some(entry))
}

fn manifest_allows_absent_binary(
    manifest: &HostBinariesManifest,
    name: HostBinaryName,
    condition: HostBinaryAbsentWhen,
) -> Result<bool, PreflightError> {
    let mut matches = manifest
        .conditional_binaries
        .iter()
        .filter(|entry| entry.name == name);
    let Some(entry) = matches.next() else {
        return Ok(false);
    };
    if matches.next().is_some() {
        return Err(PreflightError::HostBinaryDuplicate {
            name: name.as_str(),
        });
    }
    Ok(entry.absent_when == condition)
}

fn one_host_launch_material_entry(
    manifest: &HostBinariesManifest,
    name: HostLaunchMaterialName,
) -> Result<&HostLaunchMaterialEntry, PreflightError> {
    let mut matches = manifest
        .launch_material
        .iter()
        .filter(|entry| entry.name == name);
    let Some(entry) = matches.next() else {
        return Err(PreflightError::HostLaunchMaterialMissing {
            name: name.as_str(),
        });
    };
    if matches.next().is_some() {
        return Err(PreflightError::HostLaunchMaterialDuplicate {
            name: name.as_str(),
        });
    }
    Ok(entry)
}

fn verify_host_binary_entry(entry: &HostBinaryEntry) -> Result<(), PreflightError> {
    if !entry.path.is_absolute() {
        return Err(PreflightError::NonAbsolutePath {
            kind: format!("host binary {}", entry.name.as_str()),
            path: entry.path.clone(),
        });
    }
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(O_NOFOLLOW)
        .open(&entry.path)
        .map_err(|source| PreflightError::PathIo {
            path: entry.path.clone(),
            source,
        })?;
    verify_host_binary_permissions(entry, &file)?;
    verify_host_binary_sha256(entry, &mut file)
}

fn verify_host_launch_material_entry(
    entry: &HostLaunchMaterialEntry,
) -> Result<(), PreflightError> {
    if !entry.path.is_absolute() {
        return Err(PreflightError::NonAbsolutePath {
            kind: format!("host launch material {}", entry.name.as_str()),
            path: entry.path.clone(),
        });
    }
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(O_NOFOLLOW)
        .open(&entry.path)
        .map_err(|source| PreflightError::PathIo {
            path: entry.path.clone(),
            source,
        })?;
    verify_host_launch_material_permissions(entry, &file)?;
    verify_host_launch_material_sha256(entry, &mut file)
}

fn verify_host_binary_permissions(
    entry: &HostBinaryEntry,
    file: &File,
) -> Result<(), PreflightError> {
    let metadata = file.metadata().map_err(|source| PreflightError::PathIo {
        path: entry.path.clone(),
        source,
    })?;
    if !metadata.is_file() {
        return Err(PreflightError::HostBinaryPermission {
            name: entry.name.as_str(),
            path: entry.path.clone(),
            reason: "not a regular file",
        });
    }
    if metadata.uid() != 0 || metadata.gid() != 0 {
        return Err(PreflightError::HostBinaryPermission {
            name: entry.name.as_str(),
            path: entry.path.clone(),
            reason: "owner is not root:root",
        });
    }
    let mode = metadata.permissions().mode() & 0o7777;
    if mode & 0o111 == 0 {
        return Err(PreflightError::HostBinaryPermission {
            name: entry.name.as_str(),
            path: entry.path.clone(),
            reason: "not executable",
        });
    }
    if mode > 0o755 || mode & 0o022 != 0 {
        return Err(PreflightError::HostBinaryPermission {
            name: entry.name.as_str(),
            path: entry.path.clone(),
            reason: "mode is broader than 0755 or group/world-writable",
        });
    }
    Ok(())
}

fn verify_host_launch_material_permissions(
    entry: &HostLaunchMaterialEntry,
    file: &File,
) -> Result<(), PreflightError> {
    let metadata = file.metadata().map_err(|source| PreflightError::PathIo {
        path: entry.path.clone(),
        source,
    })?;
    if !metadata.is_file() {
        return Err(PreflightError::HostLaunchMaterialPermission {
            name: entry.name.as_str(),
            path: entry.path.clone(),
            reason: "not a regular file",
        });
    }
    if metadata.len() == 0 {
        return Err(PreflightError::HostLaunchMaterialPermission {
            name: entry.name.as_str(),
            path: entry.path.clone(),
            reason: "empty file",
        });
    }
    if metadata.uid() != 0 || metadata.gid() != 0 {
        return Err(PreflightError::HostLaunchMaterialPermission {
            name: entry.name.as_str(),
            path: entry.path.clone(),
            reason: "owner is not root:root",
        });
    }
    let mode = metadata.permissions().mode() & 0o7777;
    if mode > 0o755 || mode & 0o022 != 0 {
        return Err(PreflightError::HostLaunchMaterialPermission {
            name: entry.name.as_str(),
            path: entry.path.clone(),
            reason: "mode is broader than 0755 or group/world-writable",
        });
    }
    Ok(())
}

fn verify_host_binary_sha256(
    entry: &HostBinaryEntry,
    file: &mut File,
) -> Result<(), PreflightError> {
    let actual = file_sha256(&entry.path, file)?;
    if actual != entry.sha256 {
        return Err(PreflightError::BinaryHashMismatch {
            name: entry.name.as_str(),
            path: entry.path.clone(),
            expected: entry.sha256.clone(),
            actual,
        });
    }
    Ok(())
}

fn verify_host_launch_material_sha256(
    entry: &HostLaunchMaterialEntry,
    file: &mut File,
) -> Result<(), PreflightError> {
    let actual = file_sha256(&entry.path, file)?;
    if actual != entry.sha256 {
        return Err(PreflightError::HostLaunchMaterialHashMismatch {
            name: entry.name.as_str(),
            path: entry.path.clone(),
            expected: entry.sha256.clone(),
            actual,
        });
    }
    Ok(())
}

fn verify_host_binary_version(
    entry: &HostBinaryEntry,
    expected_version: Option<&str>,
) -> Result<(), PreflightError> {
    let actual = match expected_version {
        Some(version) => version.to_owned(),
        None => host_binary_version(entry.name, &entry.path)?,
    };
    if entry.version != actual {
        return Err(PreflightError::HostBinaryVersionMismatch {
            name: entry.name.as_str(),
            path: entry.path.clone(),
            expected: entry.version.clone(),
            actual,
        });
    }
    Ok(())
}

fn verify_host_launch_material_version(
    entry: &HostLaunchMaterialEntry,
    actual: &str,
) -> Result<(), PreflightError> {
    if entry.version != actual {
        return Err(PreflightError::HostLaunchMaterialVersionMismatch {
            name: entry.name.as_str(),
            path: entry.path.clone(),
            expected: entry.version.clone(),
            actual: actual.to_owned(),
        });
    }
    Ok(())
}

fn record_host_binary(
    name: HostBinaryName,
    path: &Path,
    version: String,
) -> Result<HostBinaryEntry, PreflightError> {
    require_absolute_binary(name.as_str(), path)?;
    let mut file = open_no_follow(path)?;
    Ok(HostBinaryEntry {
        name,
        path: path.to_path_buf(),
        sha256: file_sha256(path, &mut file)?,
        version,
    })
}

fn record_host_launch_material(
    name: HostLaunchMaterialName,
    path: &Path,
    version: String,
) -> Result<HostLaunchMaterialEntry, PreflightError> {
    require_absolute_binary(name.as_str(), path)?;
    let mut file = open_no_follow(path)?;
    Ok(HostLaunchMaterialEntry {
        name,
        path: path.to_path_buf(),
        sha256: file_sha256(path, &mut file)?,
        version,
    })
}

fn open_no_follow(path: &Path) -> Result<File, PreflightError> {
    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(O_NOFOLLOW)
        .open(path)
        .map_err(|source| PreflightError::PathIo {
            path: path.to_path_buf(),
            source,
        })
}

fn file_sha256(path: &Path, file: &mut File) -> Result<String, PreflightError> {
    file.seek(SeekFrom::Start(0))
        .map_err(|source| PreflightError::PathIo {
            path: path.to_path_buf(),
            source,
        })?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|source| PreflightError::PathIo {
                path: path.to_path_buf(),
                source,
            })?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

fn require_absolute_binary(kind: &str, path: &Path) -> Result<(), PreflightError> {
    if !path.is_absolute() {
        return Err(PreflightError::NonAbsolutePath {
            kind: kind.to_owned(),
            path: path.to_path_buf(),
        });
    }
    Ok(())
}

fn firecracker_version(bin: &std::path::Path) -> Result<String, PreflightError> {
    let out = version_command_output(bin)?;

    if !out.status.success() {
        return Err(PreflightError::FirecrackerVersionCommandFailed {
            path: bin.to_path_buf(),
            status: out.status.to_string(),
        });
    }

    let stdout = String::from_utf8_lossy(&out.stdout);
    parse_firecracker_version_output(&stdout)
}

fn jailer_version(bin: &std::path::Path) -> Result<String, PreflightError> {
    let out = version_command_output(bin)?;

    if !out.status.success() {
        return Err(PreflightError::JailerVersionCommandFailed {
            path: bin.to_path_buf(),
            status: out.status.to_string(),
        });
    }

    let stdout = String::from_utf8_lossy(&out.stdout);
    parse_jailer_version_output(&stdout)
}

fn host_binary_version(
    name: HostBinaryName,
    bin: &std::path::Path,
) -> Result<String, PreflightError> {
    let out = version_command_output(bin)?;

    if !out.status.success() {
        return Err(PreflightError::HostBinaryVersionCommandFailed {
            name: name.as_str(),
            path: bin.to_path_buf(),
            status: out.status.to_string(),
        });
    }

    Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

fn version_command_output(bin: &std::path::Path) -> Result<std::process::Output, PreflightError> {
    // Retry up to 5 times on ETXTBSY (binary still being written to disk).
    let mut last_text_busy = None;
    Ok('retry: {
        for attempt in 0..5 {
            match Command::new(bin).arg("--version").output() {
                Ok(out) => break 'retry out,
                Err(source) if source.raw_os_error() == Some(nix::libc::ETXTBSY) && attempt < 4 => {
                    last_text_busy = Some(source);
                    thread::sleep(Duration::from_millis(10));
                }
                Err(source) => {
                    return Err(PreflightError::PathIo {
                        path: bin.to_path_buf(),
                        source,
                    });
                }
            }
        }
        return Err(PreflightError::PathIo {
            path: bin.to_path_buf(),
            source: last_text_busy.expect("ETXTBSY retry loop records the last error"),
        });
    })
}

#[cfg(test)]
mod tests;
