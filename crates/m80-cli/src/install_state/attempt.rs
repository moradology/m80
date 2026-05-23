use std::fs;
use std::path::{Path, PathBuf};

use m80_firecracker::{ConfigError, FcError};
use serde::{Deserialize, Serialize};

const ATTEMPT_SCHEMA_VERSION: u32 = 1;
const LAST_INSTALL_ATTEMPT_NAME: &str = "last-install-attempt.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct InstallAttemptReport {
    pub(crate) path: PathBuf,
    pub(crate) status: InstallAttemptStatus,
    pub(crate) attempt: Option<InstallAttemptMetadata>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum InstallAttemptStatus {
    Missing,
    Present,
    Invalid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InstallAttemptMetadata {
    pub(crate) schema_version: u32,
    pub(crate) attempt_type: InstallAttemptType,
    pub(crate) target_tag: Option<String>,
    pub(crate) failure_stage: Option<String>,
    pub(crate) repair_command: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum InstallAttemptType {
    SuccessfulUpgrade,
    VerificationFailed,
    DowngradeRefused,
    RollbackUnsupported,
}

impl InstallAttemptMetadata {
    pub(crate) fn new(
        attempt_type: InstallAttemptType,
        target_tag: Option<String>,
        failure_stage: Option<&'static str>,
        repair_command: Option<String>,
    ) -> Self {
        Self {
            schema_version: ATTEMPT_SCHEMA_VERSION,
            attempt_type,
            target_tag,
            failure_stage: failure_stage.map(str::to_owned),
            repair_command,
        }
    }
}

pub(crate) fn last_install_attempt_path(install_root: &Path) -> PathBuf {
    install_root.join(LAST_INSTALL_ATTEMPT_NAME)
}

pub(crate) fn read_last_install_attempt(install_root: &Path) -> InstallAttemptReport {
    let path = last_install_attempt_path(install_root);
    let raw = match fs::read(&path) {
        Ok(raw) => raw,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
            return InstallAttemptReport {
                path,
                status: InstallAttemptStatus::Missing,
                attempt: None,
            };
        }
        Err(_) => {
            return InstallAttemptReport {
                path,
                status: InstallAttemptStatus::Invalid,
                attempt: None,
            };
        }
    };
    let attempt = match serde_json::from_slice::<InstallAttemptMetadata>(&raw) {
        Ok(attempt) if attempt.schema_version == ATTEMPT_SCHEMA_VERSION => attempt,
        Ok(_) | Err(_) => {
            return InstallAttemptReport {
                path,
                status: InstallAttemptStatus::Invalid,
                attempt: None,
            };
        }
    };
    InstallAttemptReport {
        path,
        status: InstallAttemptStatus::Present,
        attempt: Some(attempt),
    }
}

pub(crate) fn write_last_install_attempt(
    install_root: &Path,
    attempt: &InstallAttemptMetadata,
) -> Result<(), FcError> {
    if std::env::var_os("M80_INSTALL_INJECT_ATTEMPT_METADATA_FAILURE").is_some() {
        return Err(FcError::Config(ConfigError::InvalidValue {
            field: "install.attempt_metadata",
            reason: "injected attempt metadata failure".to_owned(),
        }));
    }
    fs::create_dir_all(install_root).map_err(|source| FcError::PathIo {
        path: install_root.to_path_buf(),
        source,
    })?;
    let path = last_install_attempt_path(install_root);
    let temp_path = install_root.join(format!(".last-install-attempt.{}.tmp", std::process::id()));
    let mut encoded = serde_json::to_vec_pretty(attempt).map_err(|source| FcError::Json {
        context: "install attempt metadata",
        source,
    })?;
    encoded.push(b'\n');
    fs::write(&temp_path, encoded).map_err(|source| FcError::PathIo {
        path: temp_path.clone(),
        source,
    })?;
    fs::rename(&temp_path, &path).map_err(|source| FcError::PathIo { path, source })?;
    Ok(())
}
