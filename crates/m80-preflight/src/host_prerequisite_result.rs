//! Versioned host-prerequisite proof result.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::{CheckRow, Discovery};

mod check_id;
mod failure;

pub use check_id::HostPrerequisiteCheckId;
pub use failure::HostPrerequisiteFailureKind;

/// Current schema version for [`HostPrerequisiteResult`].
pub const HOST_PREREQUISITE_RESULT_SCHEMA_VERSION: u32 = 1;

/// Machine-readable host-prerequisite proof shared by install, preflight,
/// diagnostics, and release proof artifacts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostPrerequisiteResult {
    /// Schema version for fail-closed readers.
    pub schema_version: u32,
    /// Ordered prerequisite checks.
    pub checks: Vec<HostPrerequisiteCheck>,
}

impl HostPrerequisiteResult {
    /// Build a schema-current result from already structured checks.
    #[must_use]
    pub fn new(checks: Vec<HostPrerequisiteCheck>) -> Self {
        Self {
            schema_version: HOST_PREREQUISITE_RESULT_SCHEMA_VERSION,
            checks,
        }
    }

    /// Build a schema-current success result from preflight table rows.
    pub fn from_success_rows(rows: &[CheckRow]) -> Result<Self, HostPrerequisiteResultError> {
        Ok(Self::new(
            rows.iter()
                .map(HostPrerequisiteCheck::from_success_row)
                .collect::<Result<Vec<_>, _>>()?,
        ))
    }

    /// Build a schema-current result from a successful full preflight
    /// discovery, preserving structured fields that table rows only render as
    /// human text.
    pub fn from_discovery(discovery: &Discovery) -> Result<Self, HostPrerequisiteResultError> {
        let mut result = Self::from_success_rows(&discovery.report)?;
        for check in &mut result.checks {
            match check.check_id {
                HostPrerequisiteCheckId::FirecrackerBinary => {
                    check.final_path = Some(discovery.firecracker_bin.clone());
                    check.expected_version =
                        Some(discovery.manifest.expected_firecracker_version.clone());
                    check.actual_version = Some(discovery.firecracker_version.clone());
                }
                HostPrerequisiteCheckId::JailerBinary => {
                    check.final_path = Some(discovery.jailer_bin.clone());
                    check.expected_version =
                        Some(discovery.manifest.expected_firecracker_version.clone());
                    check.actual_version = Some(discovery.jailer_version.clone());
                }
                HostPrerequisiteCheckId::FirecrackerSeccompFilter => {
                    check.final_path = Some(discovery.firecracker_seccomp_filter.clone());
                }
                HostPrerequisiteCheckId::JailerHardeningWrapper => {
                    check.final_path = Some(discovery.jailer_harden_bin.clone());
                }
                HostPrerequisiteCheckId::NetworkHelper => {
                    check.final_path = Some(discovery.net_helper_bin.clone());
                }
                HostPrerequisiteCheckId::KernelImage => {
                    check.final_path = Some(discovery.kernel.clone());
                }
                HostPrerequisiteCheckId::RootfsManifest => {
                    check.final_path = Some(discovery.rootfs.clone());
                }
                HostPrerequisiteCheckId::RunRoot => {
                    check.final_path = Some(discovery.run_root.clone());
                }
                _ => {}
            }
        }
        Ok(result)
    }

    /// Decode and validate a JSON proof result.
    pub fn from_json_slice(bytes: &[u8]) -> Result<Self, HostPrerequisiteResultError> {
        let result: Self =
            serde_json::from_slice(bytes).map_err(HostPrerequisiteResultError::Json)?;
        result.validate()?;
        Ok(result)
    }

    /// Validate schema version and failure remediation invariants.
    pub fn validate(&self) -> Result<(), HostPrerequisiteResultError> {
        if self.schema_version != HOST_PREREQUISITE_RESULT_SCHEMA_VERSION {
            return Err(HostPrerequisiteResultError::UnsupportedSchemaVersion {
                expected: HOST_PREREQUISITE_RESULT_SCHEMA_VERSION,
                actual: self.schema_version,
            });
        }

        for check in &self.checks {
            if check.status != HostPrerequisiteStatus::Fail {
                continue;
            }
            if check.failure_variant.is_none() {
                return Err(HostPrerequisiteResultError::MissingFailureVariant {
                    check_name: check.check_name.clone(),
                });
            }
            let Some(remediation) = &check.remediation else {
                return Err(HostPrerequisiteResultError::MissingRemediationToken {
                    check_name: check.check_name.clone(),
                });
            };
            if remediation.id.trim().is_empty() {
                return Err(HostPrerequisiteResultError::MissingRemediationToken {
                    check_name: check.check_name.clone(),
                });
            }
            let has_command = remediation
                .command
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty());
            let has_policy_link = remediation
                .policy_link
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty());
            if !has_command && !has_policy_link {
                return Err(HostPrerequisiteResultError::MissingRemediationTarget {
                    check_name: check.check_name.clone(),
                });
            }
        }

        Ok(())
    }
}

/// One host-prerequisite check in a [`HostPrerequisiteResult`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostPrerequisiteCheck {
    /// Stable machine-readable check identity.
    pub check_id: HostPrerequisiteCheckId,
    /// Stable human-readable check name.
    pub check_name: String,
    /// Final host path observed or verified by the check.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub final_path: Option<PathBuf>,
    /// Expected version, when the check has a version source of truth.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_version: Option<String>,
    /// Actual observed version, when the check probes one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actual_version: Option<String>,
    /// Expected sha256, when the check has installed-byte identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_sha256: Option<String>,
    /// Actual observed sha256, when the check hashes a file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actual_sha256: Option<String>,
    /// Expected Unix mode bits, when the check constrains mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_mode: Option<u32>,
    /// Actual Unix mode bits observed on disk.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actual_mode: Option<u32>,
    /// Expected owner, when the check constrains ownership.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_owner: Option<HostPrerequisiteOwner>,
    /// Actual owner observed on disk.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actual_owner: Option<HostPrerequisiteOwner>,
    /// Pass/fail status.
    pub status: HostPrerequisiteStatus,
    /// Typed failure variant for failed checks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_variant: Option<HostPrerequisiteFailureKind>,
    /// Stable remediation token and command or policy link for failed checks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remediation: Option<HostPrerequisiteRemediation>,
}

impl HostPrerequisiteCheck {
    /// Build a passing check.
    #[must_use]
    pub fn pass(check_id: HostPrerequisiteCheckId) -> Self {
        Self {
            check_id,
            check_name: check_id.check_name().to_string(),
            final_path: None,
            expected_version: None,
            actual_version: None,
            expected_sha256: None,
            actual_sha256: None,
            expected_mode: None,
            actual_mode: None,
            expected_owner: None,
            actual_owner: None,
            status: HostPrerequisiteStatus::Pass,
            failure_variant: None,
            remediation: None,
        }
    }

    /// Build a failing check.
    #[must_use]
    pub fn fail(
        check_id: HostPrerequisiteCheckId,
        failure_variant: HostPrerequisiteFailureKind,
        remediation: HostPrerequisiteRemediation,
    ) -> Self {
        Self {
            status: HostPrerequisiteStatus::Fail,
            failure_variant: Some(failure_variant),
            remediation: Some(remediation),
            ..Self::pass(check_id)
        }
    }

    /// Override the human label while preserving the stable machine identity.
    #[must_use]
    pub fn with_check_name(mut self, check_name: impl Into<String>) -> Self {
        self.check_name = check_name.into();
        self
    }

    /// Attach a final host path.
    #[must_use]
    pub fn with_final_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.final_path = Some(path.into());
        self
    }

    /// Attach expected and actual version facts.
    #[must_use]
    pub fn with_versions(mut self, expected: impl Into<String>, actual: impl Into<String>) -> Self {
        self.expected_version = Some(expected.into());
        self.actual_version = Some(actual.into());
        self
    }

    /// Attach expected and actual sha256 facts.
    #[must_use]
    pub fn with_sha256(mut self, expected: impl Into<String>, actual: impl Into<String>) -> Self {
        self.expected_sha256 = Some(expected.into());
        self.actual_sha256 = Some(actual.into());
        self
    }

    /// Attach expected and actual mode facts.
    #[must_use]
    pub fn with_modes(mut self, expected: u32, actual: u32) -> Self {
        self.expected_mode = Some(expected);
        self.actual_mode = Some(actual);
        self
    }

    /// Attach expected and actual owner facts.
    #[must_use]
    pub fn with_owners(
        mut self,
        expected: HostPrerequisiteOwner,
        actual: HostPrerequisiteOwner,
    ) -> Self {
        self.expected_owner = Some(expected);
        self.actual_owner = Some(actual);
        self
    }

    fn from_success_row(row: &CheckRow) -> Result<Self, HostPrerequisiteResultError> {
        if !row.passed {
            return Err(HostPrerequisiteResultError::UnexpectedFailedSuccessRow {
                check_name: row.label.clone(),
            });
        }
        Ok(Self::pass(row.check_id).with_check_name(row.label.clone()))
    }
}

/// Unix uid/gid owner fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostPrerequisiteOwner {
    /// Numeric uid.
    pub uid: u32,
    /// Numeric gid.
    pub gid: u32,
}

/// Pass/fail status for one prerequisite check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostPrerequisiteStatus {
    /// Check passed.
    Pass,
    /// Check failed and must carry failure/remediation fields.
    Fail,
}

/// Remediation attached to a failing host-prerequisite check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostPrerequisiteRemediation {
    /// Stable remediation token.
    pub id: String,
    /// Exact command to run, when m80 can name one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// Policy or runbook link, when the fix is operator-specific.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy_link: Option<String>,
}

impl HostPrerequisiteRemediation {
    /// Build remediation that points at a command.
    #[must_use]
    pub fn command(id: impl Into<String>, command: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            command: Some(command.into()),
            policy_link: None,
        }
    }

    /// Build remediation that points at a policy/runbook.
    #[must_use]
    pub fn policy_link(id: impl Into<String>, policy_link: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            command: None,
            policy_link: Some(policy_link.into()),
        }
    }
}

/// Host-prerequisite result decode or validation error.
#[derive(Debug, thiserror::Error)]
pub enum HostPrerequisiteResultError {
    /// JSON did not match the schema.
    #[error("host prerequisite result json: {0}")]
    Json(#[from] serde_json::Error),
    /// Schema version is unsupported.
    #[error(
        "unsupported host prerequisite result schema version: expected {expected}, got {actual}"
    )]
    UnsupportedSchemaVersion {
        /// Current supported schema version.
        expected: u32,
        /// Actual decoded schema version.
        actual: u32,
    },
    /// Failed check did not carry a typed failure variant.
    #[error("host prerequisite check {check_name:?} missing failure variant")]
    MissingFailureVariant {
        /// Check name.
        check_name: String,
    },
    /// Failed check did not carry a stable remediation token.
    #[error("host prerequisite check {check_name:?} missing remediation token")]
    MissingRemediationToken {
        /// Check name.
        check_name: String,
    },
    /// Failed check did not carry a command or policy link.
    #[error("host prerequisite check {check_name:?} missing remediation command or policy link")]
    MissingRemediationTarget {
        /// Check name.
        check_name: String,
    },
    /// Success-row projection received a failed table row.
    #[error("host prerequisite success row {check_name:?} was marked failed")]
    UnexpectedFailedSuccessRow {
        /// Check name.
        check_name: String,
    },
}
