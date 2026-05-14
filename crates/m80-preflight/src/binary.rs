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
use crate::PreflightError;
use m80_image_manifest::{HostBinariesManifest, HostBinaryEntry, HostBinaryName};
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

/// Default Firecracker binary location when no env override is present.
pub const DEFAULT_FIRECRACKER_BIN: &str = "/opt/firecracker/bin/firecracker";
/// Default Firecracker advanced seccomp filter location.
pub const DEFAULT_FIRECRACKER_SECCOMP_FILTER: &str =
    "/opt/firecracker/bin/firecracker-seccomp-filter.json";
/// Default jailer binary location when no env override is present.
pub(crate) const DEFAULT_JAILER_BIN: &str = "/opt/firecracker/bin/jailer";
/// Default m80 jailer hardening wrapper location when no env override is present.
pub(crate) const DEFAULT_JAILER_HARDEN_BIN: &str = "/opt/m80/bin/m80-jailer-harden";

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
            expected_firecracker_version: env::var(ENV_FIRECRACKER_VERSION).ok(),
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
    /// Resolved m80 jailer hardening wrapper path.
    pub(crate) jailer_harden_bin: PathBuf,
}

/// Resolve Firecracker and jailer binaries and fail closed on version mismatch.
pub(crate) fn discover_binaries(
    config: &BinaryDiscoveryConfig,
    cached_firecracker_version: Option<&str>,
) -> Result<BinaryDiscovery, PreflightError> {
    require_absolute_binary("firecracker", &config.firecracker_bin)?;
    require_absolute_binary(
        "firecracker seccomp filter",
        &config.firecracker_seccomp_filter,
    )?;
    require_absolute_binary("jailer", &config.jailer_bin)?;
    require_absolute_binary("m80-jailer-harden", &config.jailer_harden_bin)?;

    if !config.firecracker_bin.exists() {
        return Err(PreflightError::FirecrackerBinaryNotFound);
    }
    verify_seccomp_filter_path(&config.firecracker_seccomp_filter)?;

    let actual_version = match cached_firecracker_version {
        Some(version) => version.to_owned(),
        None => firecracker_version(&config.firecracker_bin)?,
    };
    verify_firecracker_cve_floor(&actual_version)?;
    if let Some(expected) = &config.expected_firecracker_version {
        if &actual_version != expected {
            return Err(PreflightError::FirecrackerVersionMismatch {
                expected: expected.clone(),
                actual: actual_version,
            });
        }
    }

    if !config.jailer_bin.exists() {
        return Err(PreflightError::JailerBinaryNotFound);
    }
    if !config.jailer_harden_bin.exists() {
        return Err(PreflightError::JailerHardenBinaryNotFound);
    }

    Ok(BinaryDiscovery {
        firecracker_bin: config.firecracker_bin.clone(),
        firecracker_seccomp_filter: config.firecracker_seccomp_filter.clone(),
        firecracker_version: actual_version,
        jailer_bin: config.jailer_bin.clone(),
        jailer_harden_bin: config.jailer_harden_bin.clone(),
    })
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
    manifest_path: &Path,
) -> Result<(), PreflightError> {
    let manifest =
        HostBinariesManifest::read(manifest_path).map_err(PreflightError::HostBinaryManifest)?;
    for (name, configured_path) in [
        (
            HostBinaryName::Firecracker,
            Some(config.firecracker_bin.as_path()),
        ),
        (HostBinaryName::Jailer, Some(config.jailer_bin.as_path())),
        (
            HostBinaryName::M80JailerHarden,
            Some(config.jailer_harden_bin.as_path()),
        ),
        (HostBinaryName::M80, None),
        (HostBinaryName::M80Cli, None),
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
    }
    Ok(())
}

fn one_host_binary_entry(
    manifest: &HostBinariesManifest,
    name: HostBinaryName,
) -> Result<&HostBinaryEntry, PreflightError> {
    let mut matches = manifest.binaries.iter().filter(|entry| entry.name == name);
    let Some(entry) = matches.next() else {
        return Err(PreflightError::HostBinaryMissing {
            name: name.as_str(),
        });
    };
    if matches.next().is_some() {
        return Err(PreflightError::HostBinaryDuplicate {
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
    if mode > 0o755 || mode & 0o022 != 0 {
        return Err(PreflightError::HostBinaryPermission {
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
    file.seek(SeekFrom::Start(0))
        .map_err(|source| PreflightError::PathIo {
            path: entry.path.clone(),
            source,
        })?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|source| PreflightError::PathIo {
                path: entry.path.clone(),
                source,
            })?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let actual = hex::encode(hasher.finalize());
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
    // Retry up to 5 times on ETXTBSY (binary still being written to disk).
    let mut last_text_busy = None;
    let out = 'retry: {
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
    };

    if !out.status.success() {
        return Err(PreflightError::FirecrackerVersionCommandFailed {
            path: bin.to_path_buf(),
            status: out.status.to_string(),
        });
    }

    let stdout = String::from_utf8_lossy(&out.stdout);
    let first_line = stdout.lines().next().unwrap_or("").trim();
    Ok(first_line
        .split_whitespace()
        .last()
        .unwrap_or(first_line)
        .to_string())
}

#[cfg(test)]
mod tests;
