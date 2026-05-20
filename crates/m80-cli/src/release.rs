//! Build-time release identity for the CLI.

use serde::Serialize;

/// Cargo package version compiled into the binary.
pub(crate) const PACKAGE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Optional GitHub release tag injected by release packaging.
pub(crate) const RELEASE_TAG: Option<&str> = option_env!("M80_RELEASE_TAG");

/// Optional source commit injected by release packaging.
pub(crate) const SOURCE_COMMIT: Option<&str> = option_env!("M80_RELEASE_COMMIT");

/// Optional Rust target triple injected by release packaging.
pub(crate) const TARGET_TRIPLE: Option<&str> = option_env!("M80_RELEASE_TARGET_TRIPLE");

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
    /// Source commit used for official release builds.
    pub(crate) source_commit: Option<String>,
    /// Release target for this binary, matching bundle metadata target values.
    pub(crate) target: String,
    /// Rust target triple used to build official release binaries.
    pub(crate) target_triple: Option<String>,
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
        Self::from_parts(PACKAGE_VERSION, RELEASE_TAG, SOURCE_COMMIT)
    }

    /// Build a release identity from explicit inputs for tests.
    pub(crate) fn from_parts(
        package_version: &str,
        release_tag: Option<&str>,
        source_commit: Option<&str>,
    ) -> Self {
        let expected_release_tag = expected_tag(package_version);
        let release_tag_owned = release_tag.map(str::to_owned);
        let source_commit_owned = source_commit.map(str::to_owned);
        let target_triple_owned = TARGET_TRIPLE.map(str::to_owned);
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
            source_commit: source_commit_owned,
            target: release_target(),
            target_triple: target_triple_owned,
        }
    }
}

fn expected_tag(package_version: &str) -> String {
    format!("v{package_version}")
}

fn release_target() -> String {
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expected_current_target() -> String {
        format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
    }

    #[test]
    fn dev_build_is_marked_unreleased() {
        let identity = VersionIdentity::from_parts("1.2.3", None, None);

        assert_eq!(identity.binary_version, "1.2.3-dev");
        assert_eq!(identity.package_version, "1.2.3");
        assert_eq!(identity.release_tag, None);
        assert!(!identity.release_build);
        assert_eq!(identity.version_status, VersionStatus::Dev);
        assert_eq!(identity.expected_release_tag, "v1.2.3");
        assert_eq!(identity.source_commit, None);
        assert_eq!(identity.target, expected_current_target());
        assert_eq!(identity.target_triple, None);
    }

    #[test]
    fn release_build_uses_exact_tag_as_binary_version() {
        let identity = VersionIdentity::from_parts(
            "1.2.3",
            Some("v1.2.3"),
            Some("0123456789abcdef0123456789abcdef01234567"),
        );

        assert_eq!(identity.binary_version, "v1.2.3");
        assert_eq!(identity.release_tag.as_deref(), Some("v1.2.3"));
        assert!(identity.release_build);
        assert_eq!(identity.version_status, VersionStatus::Release);
        assert_eq!(
            identity.source_commit.as_deref(),
            Some("0123456789abcdef0123456789abcdef01234567")
        );
        assert_eq!(identity.target, expected_current_target());
    }

    #[test]
    fn mismatched_release_tag_is_visible() {
        let identity = VersionIdentity::from_parts("1.2.3", Some("v9.9.9"), None);

        assert_eq!(identity.binary_version, "v9.9.9");
        assert_eq!(identity.expected_release_tag, "v1.2.3");
        assert!(identity.release_build);
        assert_eq!(identity.version_status, VersionStatus::Mismatch);
    }
}
