//! Build-time release identity for the CLI.

use serde::Serialize;

/// Cargo package version compiled into the binary.
pub(crate) const PACKAGE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Optional GitHub release tag injected by release packaging.
pub(crate) const RELEASE_TAG: Option<&str> = option_env!("M80_RELEASE_TAG");

/// User-visible version string for clap's `--version` output.
pub(crate) const DISPLAY_VERSION: &str = match RELEASE_TAG {
    Some(tag) => tag,
    None => concat!(env!("CARGO_PKG_VERSION"), "-dev"),
};

/// Version identity emitted by `m80 version`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct VersionIdentity {
    /// User-visible binary version.
    pub(crate) binary_version: String,
    /// Cargo package version compiled into the binary.
    pub(crate) package_version: String,
    /// GitHub release tag injected by release packaging.
    pub(crate) release_tag: Option<String>,
    /// Whether this binary was built with release tag metadata.
    pub(crate) release_build: bool,
    /// Machine-readable status: `release`, `dev`, or `mismatch`.
    pub(crate) version_status: VersionStatus,
    /// Expected release tag for this Cargo package version.
    pub(crate) expected_release_tag: String,
}

/// Machine-readable release-version status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum VersionStatus {
    /// Release tag was injected and matches `v<CARGO_PKG_VERSION>`.
    Release,
    /// No release tag was injected.
    Dev,
    /// Release tag was injected but does not match `v<CARGO_PKG_VERSION>`.
    Mismatch,
}

impl VersionStatus {
    /// Stable text form for human output.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            VersionStatus::Release => "release",
            VersionStatus::Dev => "dev",
            VersionStatus::Mismatch => "mismatch",
        }
    }
}

impl VersionIdentity {
    /// Return the build identity for this binary.
    pub(crate) fn current() -> Self {
        Self::from_parts(PACKAGE_VERSION, RELEASE_TAG)
    }

    /// Build a release identity from explicit inputs for tests.
    pub(crate) fn from_parts(package_version: &str, release_tag: Option<&str>) -> Self {
        let expected_release_tag = expected_tag(package_version);
        let release_tag_owned = release_tag.map(str::to_owned);
        let version_status = match release_tag {
            Some(tag) if tag == expected_release_tag => VersionStatus::Release,
            Some(_) => VersionStatus::Mismatch,
            None => VersionStatus::Dev,
        };
        let binary_version = release_tag_owned
            .clone()
            .unwrap_or_else(|| format!("{package_version}-dev"));

        Self {
            binary_version,
            package_version: package_version.to_owned(),
            release_tag: release_tag_owned,
            release_build: release_tag.is_some(),
            version_status,
            expected_release_tag,
        }
    }
}

fn expected_tag(package_version: &str) -> String {
    format!("v{package_version}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dev_build_is_marked_unreleased() {
        let identity = VersionIdentity::from_parts("1.2.3", None);

        assert_eq!(identity.binary_version, "1.2.3-dev");
        assert_eq!(identity.package_version, "1.2.3");
        assert_eq!(identity.release_tag, None);
        assert!(!identity.release_build);
        assert_eq!(identity.version_status, VersionStatus::Dev);
        assert_eq!(identity.expected_release_tag, "v1.2.3");
    }

    #[test]
    fn release_build_uses_exact_tag_as_binary_version() {
        let identity = VersionIdentity::from_parts("1.2.3", Some("v1.2.3"));

        assert_eq!(identity.binary_version, "v1.2.3");
        assert_eq!(identity.release_tag.as_deref(), Some("v1.2.3"));
        assert!(identity.release_build);
        assert_eq!(identity.version_status, VersionStatus::Release);
    }

    #[test]
    fn mismatched_release_tag_is_visible() {
        let identity = VersionIdentity::from_parts("1.2.3", Some("v9.9.9"));

        assert_eq!(identity.binary_version, "v9.9.9");
        assert_eq!(identity.expected_release_tag, "v1.2.3");
        assert!(identity.release_build);
        assert_eq!(identity.version_status, VersionStatus::Mismatch);
    }
}
