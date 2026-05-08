//! Firecracker and jailer binary discovery.

use std::env;
use std::path::PathBuf;
use std::process::Command;
use std::thread;
use std::time::Duration;

use crate::cve_floor::verify_firecracker_cve_floor;
use crate::PreflightError;

/// Environment key for overriding the Firecracker binary path.
pub const ENV_FIRECRACKER_BIN: &str = "M80_FIRECRACKER_BIN";
/// Environment key for enabling an exact Firecracker version check.
pub const ENV_FIRECRACKER_VERSION: &str = "M80_FIRECRACKER_VERSION";
/// Environment key for overriding the jailer binary path.
pub const ENV_JAILER_BIN: &str = "M80_JAILER_BIN";
/// Environment key for overriding the m80 jailer hardening wrapper path.
pub const ENV_JAILER_HARDEN_BIN: &str = "M80_JAILER_HARDEN_BIN";

/// Default Firecracker binary location when no env override is present.
pub const DEFAULT_FIRECRACKER_BIN: &str = "/opt/firecracker/bin/firecracker";
/// Default jailer binary location when no env override is present.
pub const DEFAULT_JAILER_BIN: &str = "/opt/firecracker/bin/jailer";
/// Default m80 jailer hardening wrapper location when no env override is present.
pub const DEFAULT_JAILER_HARDEN_BIN: &str = "/opt/m80/bin/m80-jailer-harden";

/// Inputs for the binary discovery preflight step.
#[derive(Debug, Clone)]
pub struct BinaryDiscoveryConfig {
    /// Firecracker binary path to probe with `--version`.
    pub firecracker_bin: PathBuf,
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

impl Default for BinaryDiscoveryConfig {
    fn default() -> Self {
        Self {
            firecracker_bin: PathBuf::from(DEFAULT_FIRECRACKER_BIN),
            jailer_bin: PathBuf::from(DEFAULT_JAILER_BIN),
            jailer_harden_bin: PathBuf::from(DEFAULT_JAILER_HARDEN_BIN),
            expected_firecracker_version: None,
        }
    }
}

/// Resolved binary paths and the probed Firecracker version.
#[derive(Debug, Clone)]
pub struct BinaryDiscovery {
    /// Resolved Firecracker binary path.
    pub firecracker_bin: PathBuf,
    /// Version parsed from `firecracker --version`.
    pub firecracker_version: String,
    /// Resolved jailer binary path.
    pub jailer_bin: PathBuf,
    /// Resolved m80 jailer hardening wrapper path.
    pub jailer_harden_bin: PathBuf,
}

/// Resolve Firecracker and jailer binaries and fail closed on version mismatch.
pub fn discover_binaries(
    config: &BinaryDiscoveryConfig,
) -> Result<BinaryDiscovery, PreflightError> {
    if !config.firecracker_bin.exists() {
        return Err(PreflightError::FirecrackerBinaryNotFound);
    }

    let actual_version = firecracker_version(&config.firecracker_bin)?;
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
        firecracker_version: actual_version,
        jailer_bin: config.jailer_bin.clone(),
        jailer_harden_bin: config.jailer_harden_bin.clone(),
    })
}

fn firecracker_version(bin: &std::path::Path) -> Result<String, PreflightError> {
    let out = firecracker_version_output(bin)?;

    if !out.status.success() {
        return Err(PreflightError::Io(std::io::Error::other(format!(
            "{} --version exited with {}",
            bin.display(),
            out.status
        ))));
    }

    let stdout = String::from_utf8_lossy(&out.stdout);
    let first_line = stdout.lines().next().unwrap_or("").trim();
    Ok(first_line
        .split_whitespace()
        .last()
        .unwrap_or(first_line)
        .to_string())
}

fn firecracker_version_output(
    bin: &std::path::Path,
) -> Result<std::process::Output, PreflightError> {
    let mut last_text_busy = None;
    for attempt in 0..5 {
        match Command::new(bin).arg("--version").output() {
            Ok(out) => return Ok(out),
            Err(source) if source.raw_os_error() == Some(nix::libc::ETXTBSY) && attempt < 4 => {
                last_text_busy = Some(source);
                thread::sleep(Duration::from_millis(10));
            }
            Err(source) => return Err(PreflightError::Io(source)),
        }
    }

    Err(PreflightError::Io(
        last_text_busy.expect("ETXTBSY retry loop records the last error"),
    ))
}
