//! Firecracker and official jailer release-train policy.

use crate::cve_floor::{
    active_firecracker_cve_floors, FirecrackerCveFloor, FIRECRACKER_CVE_FLOOR_SOURCE,
};
use crate::PreflightError;

/// File that owns the Firecracker train and official jailer pairing policy.
pub const FIRECRACKER_TRAIN_POLICY_SOURCE: &str = "crates/m80-preflight/src/firecracker_train.rs";

/// Operator-facing policy document for Firecracker and jailer prerequisites.
pub const HOST_PREREQUISITE_POLICY_DOC: &str = "docs/behaviors/release/host-prerequisite-policy.md";

/// Pairing rule for the official Firecracker jailer.
pub const JAILER_PAIRING_RULE: &str =
    "official jailer --version must exactly match accepted firecracker --version";

/// Checked Firecracker train policy used by preflight and release tooling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FirecrackerTrainPolicy {
    expected_firecracker_version: Option<String>,
}

impl FirecrackerTrainPolicy {
    /// Build the policy around an optional exact Firecracker version.
    #[must_use]
    pub fn from_expected_firecracker_version(expected_firecracker_version: Option<String>) -> Self {
        Self {
            expected_firecracker_version,
        }
    }

    /// Exact expected Firecracker version, when the current caller has one.
    #[must_use]
    pub fn expected_firecracker_version(&self) -> Option<&str> {
        self.expected_firecracker_version.as_deref()
    }

    /// Official jailer pairing rule enforced by preflight.
    #[must_use]
    pub fn jailer_pairing_rule(&self) -> &'static str {
        JAILER_PAIRING_RULE
    }

    /// Active Firecracker CVE floors enforced by preflight.
    #[must_use]
    pub fn cve_floor(&self) -> &'static [FirecrackerCveFloor] {
        active_firecracker_cve_floors()
    }

    /// File that owns the Firecracker train policy.
    #[must_use]
    pub fn source(&self) -> &'static str {
        FIRECRACKER_TRAIN_POLICY_SOURCE
    }

    /// File that owns the active Firecracker CVE floor table.
    #[must_use]
    pub fn cve_floor_source(&self) -> &'static str {
        FIRECRACKER_CVE_FLOOR_SOURCE
    }

    /// Operator-facing policy document for this train contract.
    #[must_use]
    pub fn policy_doc(&self) -> &'static str {
        HOST_PREREQUISITE_POLICY_DOC
    }
}

pub(crate) fn enforce_configured_firecracker_version(
    policy: &FirecrackerTrainPolicy,
    actual: &str,
) -> Result<(), PreflightError> {
    let Some(expected) = policy.expected_firecracker_version() else {
        return Ok(());
    };
    enforce_firecracker_version(expected, actual)
}

pub(crate) fn enforce_firecracker_version(
    expected: &str,
    actual: &str,
) -> Result<(), PreflightError> {
    if actual == expected {
        return Ok(());
    }

    Err(PreflightError::FirecrackerVersionMismatch {
        expected: expected.to_owned(),
        actual: actual.to_owned(),
        policy_source: FIRECRACKER_TRAIN_POLICY_SOURCE,
    })
}

pub(crate) fn enforce_jailer_pairing(
    firecracker_version: &str,
    jailer_version: &str,
) -> Result<(), PreflightError> {
    if jailer_version == firecracker_version {
        return Ok(());
    }

    Err(PreflightError::JailerVersionMismatch {
        expected: firecracker_version.to_owned(),
        actual: jailer_version.to_owned(),
        policy_source: FIRECRACKER_TRAIN_POLICY_SOURCE,
    })
}

pub(crate) fn parse_firecracker_version_output(stdout: &str) -> Result<String, PreflightError> {
    parse_version_output(stdout, "Firecracker").map_err(|actual| {
        PreflightError::FirecrackerVersionOutputMalformed {
            actual,
            policy_source: FIRECRACKER_TRAIN_POLICY_SOURCE,
        }
    })
}

pub(crate) fn parse_jailer_version_output(stdout: &str) -> Result<String, PreflightError> {
    parse_version_output(stdout, "Jailer").map_err(|actual| {
        PreflightError::JailerVersionOutputMalformed {
            actual,
            policy_source: FIRECRACKER_TRAIN_POLICY_SOURCE,
        }
    })
}

fn parse_version_output(stdout: &str, product: &str) -> Result<String, String> {
    let actual = stdout.trim().to_owned();
    let first_line = stdout.lines().next().unwrap_or("").trim();
    let mut parts = first_line.split_whitespace();
    let Some(observed_product) = parts.next() else {
        return Err(actual);
    };
    let Some(version) = parts.next() else {
        return Err(actual);
    };
    if parts.next().is_some() || observed_product != product || !is_release_version(version) {
        return Err(actual);
    }

    Ok(version.to_owned())
}

fn is_release_version(version: &str) -> bool {
    let Some(version) = version.strip_prefix('v') else {
        return false;
    };
    let mut parts = version.split('.');
    let Some(major) = parts.next() else {
        return false;
    };
    let Some(minor) = parts.next() else {
        return false;
    };
    let Some(patch) = parts.next() else {
        return false;
    };
    parts.next().is_none()
        && !major.is_empty()
        && !minor.is_empty()
        && !patch.is_empty()
        && major.chars().all(|ch| ch.is_ascii_digit())
        && minor.chars().all(|ch| ch.is_ascii_digit())
        && patch.chars().all(|ch| ch.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::{
        enforce_firecracker_version, enforce_jailer_pairing, parse_firecracker_version_output,
        parse_jailer_version_output, FirecrackerTrainPolicy, FIRECRACKER_TRAIN_POLICY_SOURCE,
        JAILER_PAIRING_RULE,
    };
    use crate::PreflightError;

    #[test]
    fn policy_returns_expected_version_pairing_rule_and_cve_floor() {
        let policy =
            FirecrackerTrainPolicy::from_expected_firecracker_version(Some("v1.15.1".into()));

        assert_eq!(policy.expected_firecracker_version(), Some("v1.15.1"));
        assert_eq!(policy.jailer_pairing_rule(), JAILER_PAIRING_RULE);
        assert!(!policy.cve_floor().is_empty());
        assert_eq!(policy.source(), FIRECRACKER_TRAIN_POLICY_SOURCE);
        assert_eq!(
            policy.cve_floor_source(),
            "crates/m80-preflight/src/cve_floor.rs"
        );
        assert_eq!(
            policy.policy_doc(),
            "docs/behaviors/release/host-prerequisite-policy.md"
        );
    }

    #[test]
    fn parses_supported_firecracker_version_output() {
        let version = parse_firecracker_version_output("Firecracker v1.15.1\n").unwrap();

        assert_eq!(version, "v1.15.1");
    }

    #[test]
    fn parses_firecracker_version_with_exit_log_after_first_line() {
        let version = parse_firecracker_version_output(
            "Firecracker v1.15.1\n\n2026-05-17T11:31:56Z [anonymous-instance:main] Firecracker exiting successfully. exit_code=0\n",
        )
        .unwrap();

        assert_eq!(version, "v1.15.1");
    }

    #[test]
    fn rejects_unsupported_firecracker_version_output() {
        let err = parse_firecracker_version_output("NotFirecracker v1.15.1\n").unwrap_err();

        assert!(matches!(
            err,
            PreflightError::FirecrackerVersionOutputMalformed { .. }
        ));
    }

    #[test]
    fn rejects_empty_firecracker_version_output() {
        let err = parse_firecracker_version_output("").unwrap_err();

        assert!(matches!(
            err,
            PreflightError::FirecrackerVersionOutputMalformed { .. }
        ));
    }

    #[test]
    fn rejects_malformed_firecracker_version_output() {
        let err = parse_firecracker_version_output("Firecracker dev-build\n").unwrap_err();

        assert!(matches!(
            err,
            PreflightError::FirecrackerVersionOutputMalformed { .. }
        ));
    }

    #[test]
    fn parses_supported_jailer_version_output() {
        let version = parse_jailer_version_output("Jailer v1.15.1\n").unwrap();

        assert_eq!(version, "v1.15.1");
    }

    #[test]
    fn rejects_unsupported_jailer_version_output() {
        let err = parse_jailer_version_output("NotJailer v1.15.1\n").unwrap_err();

        assert!(matches!(
            err,
            PreflightError::JailerVersionOutputMalformed { .. }
        ));
    }

    #[test]
    fn rejects_empty_jailer_version_output() {
        let err = parse_jailer_version_output("").unwrap_err();

        assert!(matches!(
            err,
            PreflightError::JailerVersionOutputMalformed { .. }
        ));
    }

    #[test]
    fn rejects_malformed_jailer_version_output() {
        let err = parse_jailer_version_output("Jailer 1.15.1\n").unwrap_err();

        assert!(matches!(
            err,
            PreflightError::JailerVersionOutputMalformed { .. }
        ));
    }

    #[test]
    fn firecracker_version_must_match_expected_train() {
        let err = enforce_firecracker_version("v1.15.1", "v1.14.4").unwrap_err();

        match err {
            PreflightError::FirecrackerVersionMismatch {
                expected,
                actual,
                policy_source,
            } => {
                assert_eq!(expected, "v1.15.1");
                assert_eq!(actual, "v1.14.4");
                assert_eq!(policy_source, FIRECRACKER_TRAIN_POLICY_SOURCE);
            }
            other => panic!("expected firecracker version mismatch, got {other:?}"),
        }
    }

    #[test]
    fn jailer_version_must_match_firecracker_version() {
        let err = enforce_jailer_pairing("v1.15.1", "v1.15.0").unwrap_err();

        match err {
            PreflightError::JailerVersionMismatch {
                expected,
                actual,
                policy_source,
            } => {
                assert_eq!(expected, "v1.15.1");
                assert_eq!(actual, "v1.15.0");
                assert_eq!(policy_source, FIRECRACKER_TRAIN_POLICY_SOURCE);
            }
            other => panic!("expected jailer version mismatch, got {other:?}"),
        }
    }
}
