use serde::{Deserialize, Serialize};

use crate::host_prerequisite_result::HostPrerequisiteCheckId;

/// One row in the preflight report.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckRow {
    /// Stable machine-readable check identity.
    pub check_id: HostPrerequisiteCheckId,
    /// Short label naming the check.
    pub label: String,
    /// Whether the check passed.
    pub passed: bool,
    /// Free-text detail rendered alongside the label.
    pub detail: String,
}

impl CheckRow {
    /// Build a passing row from the stable check registry.
    #[must_use]
    pub fn pass(check_id: HostPrerequisiteCheckId, detail: impl Into<String>) -> Self {
        Self {
            check_id,
            label: check_id.check_name().to_string(),
            passed: true,
            detail: detail.into(),
        }
    }

    /// Build a failing row from the stable check registry.
    #[must_use]
    pub fn fail(check_id: HostPrerequisiteCheckId, detail: impl Into<String>) -> Self {
        Self {
            check_id,
            label: check_id.check_name().to_string(),
            passed: false,
            detail: detail.into(),
        }
    }

    /// Override the human label while preserving the stable machine identity.
    #[must_use]
    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }
}
