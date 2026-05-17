//! Firecracker CVE floor checks.

use crate::PreflightError;

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
    id: &'static str,
    fixed_versions: &'static str,
    affected: fn(Version) -> bool,
}

const TRACKED_FIRECRACKER_CVES: &[TrackedCve] = &[
    TrackedCve {
        id: "CVE-2026-5747",
        fixed_versions: "v1.14.4 or v1.15.1",
        affected: |version| {
            (version >= Version::new(1, 13, 0) && version <= Version::new(1, 14, 3))
                || version == Version::new(1, 15, 0)
        },
    },
    TrackedCve {
        id: "CVE-2026-1386",
        fixed_versions: "v1.13.2 or v1.14.1",
        affected: |version| version <= Version::new(1, 13, 1) || version == Version::new(1, 14, 0),
    },
];

pub(crate) fn verify_firecracker_cve_floor(version: &str) -> Result<(), PreflightError> {
    let parsed = Version::parse(version)?;
    if let Some(cve) = TRACKED_FIRECRACKER_CVES
        .iter()
        .find(|cve| (cve.affected)(parsed))
    {
        return Err(PreflightError::FirecrackerCveFloorViolation {
            cve_id: cve.id.to_owned(),
            actual: version.to_owned(),
            fixed_versions: cve.fixed_versions.to_owned(),
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
        cve_id: "firecracker-version-format".to_owned(),
        actual: version.to_owned(),
        fixed_versions: "release versions like v1.14.4 or v1.15.1".to_owned(),
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
                fixed_versions,
            } => {
                assert_eq!(cve_id, "CVE-2026-5747");
                assert_eq!(actual, "v1.15.0");
                assert_eq!(fixed_versions, "v1.14.4 or v1.15.1");
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
                fixed_versions,
            } => {
                assert_eq!(cve_id, "firecracker-version-format");
                assert_eq!(actual, "dev-build");
                assert_eq!(fixed_versions, "release versions like v1.14.4 or v1.15.1");
            }
            other => panic!("expected CVE floor violation, got {other:?}"),
        }
    }
}
