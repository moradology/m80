//! Firecracker CVE floor checks.

use crate::PreflightError;

/// File that owns the active Firecracker CVE floor table.
pub const FIRECRACKER_CVE_FLOOR_SOURCE: &str = "crates/m80-preflight/src/cve_floor.rs";

/// One Firecracker CVE floor enforced by preflight.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FirecrackerCveFloor {
    /// CVE identifier tracked by m80.
    pub cve_id: &'static str,
    /// Fixed version set that satisfies this floor.
    pub expected: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Version {
    major: u16,
    minor: u16,
    patch: u16,
}

impl Version {
    fn new(major: u16, minor: u16, patch: u16) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    fn parse(input: &str) -> Result<Self, PreflightError> {
        let version = input.trim().strip_prefix('v').unwrap_or(input.trim());
        let mut parts = version.split('.');
        let major = parse_component(parts.next(), input)?;
        let minor = parse_component(parts.next(), input)?;
        let patch = parse_component(parts.next(), input)?;
        if parts.next().is_some() {
            return Err(format_error(input));
        }

        Ok(Self::new(major, minor, patch))
    }
}

struct TrackedCve {
    floor: FirecrackerCveFloor,
    affected: fn(Version) -> bool,
}

const CVE_2026_5747: FirecrackerCveFloor = FirecrackerCveFloor {
    cve_id: "CVE-2026-5747",
    expected: "v1.14.4 or v1.15.1",
};

const CVE_2026_1386: FirecrackerCveFloor = FirecrackerCveFloor {
    cve_id: "CVE-2026-1386",
    expected: "v1.13.2 or v1.14.1",
};

const FIRECRACKER_VERSION_FORMAT: FirecrackerCveFloor = FirecrackerCveFloor {
    cve_id: "firecracker-version-format",
    expected: "release versions like v1.14.4 or v1.15.1",
};

const ACTIVE_FIRECRACKER_CVE_FLOORS: &[FirecrackerCveFloor] = &[CVE_2026_5747, CVE_2026_1386];

const TRACKED_FIRECRACKER_CVES: &[TrackedCve] = &[
    TrackedCve {
        floor: CVE_2026_5747,
        affected: |version| {
            (version >= Version::new(1, 13, 0) && version <= Version::new(1, 14, 3))
                || version == Version::new(1, 15, 0)
        },
    },
    TrackedCve {
        floor: CVE_2026_1386,
        affected: |version| version <= Version::new(1, 13, 1) || version == Version::new(1, 14, 0),
    },
];

/// Return the active Firecracker CVE floors enforced by preflight.
#[must_use]
pub fn active_firecracker_cve_floors() -> &'static [FirecrackerCveFloor] {
    ACTIVE_FIRECRACKER_CVE_FLOORS
}

pub(crate) fn verify_firecracker_cve_floor(version: &str) -> Result<(), PreflightError> {
    let parsed = Version::parse(version)?;
    if let Some(cve) = TRACKED_FIRECRACKER_CVES
        .iter()
        .find(|cve| (cve.affected)(parsed))
    {
        return Err(PreflightError::FirecrackerCveFloorViolation {
            cve_id: cve.floor.cve_id.to_owned(),
            actual: version.to_owned(),
            expected: cve.floor.expected.to_owned(),
            policy_source: FIRECRACKER_CVE_FLOOR_SOURCE,
        });
    }

    Ok(())
}

fn parse_component(part: Option<&str>, original: &str) -> Result<u16, PreflightError> {
    part.and_then(|value| value.parse().ok())
        .ok_or_else(|| format_error(original))
}

fn format_error(version: &str) -> PreflightError {
    PreflightError::FirecrackerCveFloorViolation {
        cve_id: FIRECRACKER_VERSION_FORMAT.cve_id.to_owned(),
        actual: version.to_owned(),
        expected: FIRECRACKER_VERSION_FORMAT.expected.to_owned(),
        policy_source: FIRECRACKER_CVE_FLOOR_SOURCE,
    }
}

#[cfg(test)]
mod tests {
    use super::verify_firecracker_cve_floor;
    use crate::PreflightError;

    #[test]
    fn rejects_virtio_pci_cve_affected_v1_15_0() {
        let err = verify_firecracker_cve_floor("v1.15.0").unwrap_err();

        match err {
            PreflightError::FirecrackerCveFloorViolation {
                cve_id,
                actual,
                expected,
                policy_source,
            } => {
                assert_eq!(cve_id, "CVE-2026-5747");
                assert_eq!(actual, "v1.15.0");
                assert_eq!(expected, "v1.14.4 or v1.15.1");
                assert_eq!(policy_source, "crates/m80-preflight/src/cve_floor.rs");
            }
            other => panic!("expected CVE floor violation, got {other:?}"),
        }
    }

    #[test]
    fn rejects_jailer_cve_affected_v1_12_0() {
        let err = verify_firecracker_cve_floor("v1.12.0").unwrap_err();

        match err {
            PreflightError::FirecrackerCveFloorViolation { cve_id, actual, .. } => {
                assert_eq!(cve_id, "CVE-2026-1386");
                assert_eq!(actual, "v1.12.0");
            }
            other => panic!("expected CVE floor violation, got {other:?}"),
        }
    }

    #[test]
    fn accepts_fixed_v1_14_train_version() {
        verify_firecracker_cve_floor("v1.14.4").unwrap();
    }

    #[test]
    fn accepts_fixed_v1_15_train_version() {
        verify_firecracker_cve_floor("v1.15.1").unwrap();
    }

    #[test]
    fn accepts_future_minor_version() {
        verify_firecracker_cve_floor("v1.16.0").unwrap();
    }

    #[test]
    fn malformed_version_fails_closed() {
        let err = verify_firecracker_cve_floor("dev-build").unwrap_err();

        match err {
            PreflightError::FirecrackerCveFloorViolation {
                cve_id,
                actual,
                expected,
                policy_source,
            } => {
                assert_eq!(cve_id, "firecracker-version-format");
                assert_eq!(actual, "dev-build");
                assert_eq!(expected, "release versions like v1.14.4 or v1.15.1");
                assert_eq!(policy_source, "crates/m80-preflight/src/cve_floor.rs");
            }
            other => panic!("expected CVE floor violation, got {other:?}"),
        }
    }
}
