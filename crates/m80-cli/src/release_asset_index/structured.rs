use serde::Serialize;

use crate::release::{VersionIdentity, VersionStatus};

use super::{fetch, AssetIndexError, AssetIndexRequest};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AssetIndexFailure {
    diagnostic: AssetIndexDiagnostic,
}

impl AssetIndexFailure {
    pub(super) fn from_selection(source: AssetIndexError, requested: AssetIndexRequest) -> Self {
        Self {
            diagnostic: source.into_diagnostic(requested),
        }
    }

    pub(super) fn from_fetch(source: fetch::AssetIndexFetchError, m80_version: &str) -> Self {
        Self {
            diagnostic: source.into_diagnostic(m80_version),
        }
    }

    pub(crate) fn dev_build_refused(
        source_flag: &str,
        release_tag: &str,
        identity: &VersionIdentity,
    ) -> Self {
        let requested = AssetIndexRequest::from_current_host(release_tag, &identity.binary_version);
        Self {
            diagnostic: requested.diagnostic(
                AssetIndexDiagnosticCode::DevBuildRefused,
                format!(
                    "{source_flag} install requires a tagged m80 binary; this binary is {} ({})",
                    identity.binary_version,
                    VersionStatus::Dev.as_str()
                ),
                Vec::new(),
                Vec::new(),
                Vec::new(),
                None,
                Some("m80 install --bundle-url <compatible-bundle-url>".to_owned()),
            ),
        }
    }

    pub(crate) fn mismatched_build_refused(release_tag: &str, identity: &VersionIdentity) -> Self {
        let requested = AssetIndexRequest::from_current_host(release_tag, &identity.binary_version);
        Self {
            diagnostic: requested.diagnostic(
                AssetIndexDiagnosticCode::BinaryTagMismatch,
                format!(
                    "binary-vs-bundle mismatch: binary_version={} binary_release_tag={} bundle_version={} release_tag={} repair: curl -fsSL {} | sudo sh",
                    identity.binary_version,
                    identity.release_tag.as_deref().unwrap_or("<missing>"),
                    identity.expected_release_tag,
                    identity.expected_release_tag,
                    crate::release_urls::release_install_url(&identity.expected_release_tag)
                ),
                Vec::new(),
                Vec::new(),
                Vec::new(),
                None,
                Some(format!(
                    "curl -fsSL {} | sudo sh",
                    crate::release_urls::release_install_url(&identity.expected_release_tag)
                )),
            ),
        }
    }

    pub(crate) fn tag_mismatch_refused(
        source_tag: &str,
        binary_tag: &str,
        identity: &VersionIdentity,
    ) -> Self {
        let requested = AssetIndexRequest::from_current_host(source_tag, &identity.binary_version);
        Self {
            diagnostic: requested.diagnostic(
                AssetIndexDiagnosticCode::BinaryTagMismatch,
                format!(
                    "bundle/binary tag mismatch: binary_version={} binary_release_tag={binary_tag} bundle_version={source_tag} release_tag={source_tag} repair: curl -fsSL {} | sudo sh",
                    identity.binary_version,
                    crate::release_urls::release_install_url(source_tag)
                ),
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Some(crate::release_urls::release_install_url(binary_tag)),
                Some(format!(
                    "curl -fsSL {} | sudo sh",
                    crate::release_urls::release_install_url(binary_tag)
                )),
            ),
        }
    }

    pub(crate) fn diagnostic(&self) -> &AssetIndexDiagnostic {
        &self.diagnostic
    }
}

impl std::fmt::Display for AssetIndexFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.diagnostic.detail)
    }
}

impl std::error::Error for AssetIndexFailure {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AssetIndexDiagnostic {
    pub(crate) code: AssetIndexDiagnosticCode,
    pub(crate) detail: String,
    pub(crate) requested_os: String,
    pub(crate) requested_arch: String,
    pub(crate) requested_image_kind: String,
    pub(crate) requested_release_tag: String,
    pub(crate) requested_m80_version: String,
    pub(crate) available_tuples: Vec<String>,
    pub(crate) available_image_kinds: Vec<String>,
    pub(crate) available_m80_versions: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) index_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) fetch_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) checksum_verification: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) repair_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) repair_command: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AssetIndexDiagnosticCode {
    InvalidJson,
    UnsupportedSchema,
    MissingField,
    InvalidField,
    MissingDefaultBundle,
    DevBuildRefused,
    BinaryTagMismatch,
    IndexTagMismatch,
    UnsupportedHostTuple,
    MissingImageKind,
    StaleAssetIndex,
    DuplicateDefaultBundle,
    AssetReleaseTagMismatch,
    TargetTupleMismatch,
    UnsupportedUrl,
    LocalReadFailed,
    DownloadSpawnFailed,
    DownloadFailed,
    RedirectUnsupported,
    ChecksumInvalid,
    ChecksumMismatch,
    VerifiedIndexInvalid,
}

impl AssetIndexDiagnosticCode {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::InvalidJson => "invalid_json",
            Self::UnsupportedSchema => "unsupported_schema",
            Self::MissingField => "missing_field",
            Self::InvalidField => "invalid_field",
            Self::MissingDefaultBundle => "missing_default_bundle",
            Self::DevBuildRefused => "dev_build_refused",
            Self::BinaryTagMismatch => "binary_tag_mismatch",
            Self::IndexTagMismatch => "index_tag_mismatch",
            Self::UnsupportedHostTuple => "unsupported_host_tuple",
            Self::MissingImageKind => "missing_image_kind",
            Self::StaleAssetIndex => "stale_asset_index",
            Self::DuplicateDefaultBundle => "duplicate_default_bundle",
            Self::AssetReleaseTagMismatch => "asset_release_tag_mismatch",
            Self::TargetTupleMismatch => "target_tuple_mismatch",
            Self::UnsupportedUrl => "unsupported_url",
            Self::LocalReadFailed => "local_read_failed",
            Self::DownloadSpawnFailed => "download_spawn_failed",
            Self::DownloadFailed => "download_failed",
            Self::RedirectUnsupported => "redirect_unsupported",
            Self::ChecksumInvalid => "checksum_invalid",
            Self::ChecksumMismatch => "checksum_mismatch",
            Self::VerifiedIndexInvalid => "verified_index_invalid",
        }
    }
}

impl AssetIndexError {
    pub(super) fn diagnostic_code(&self) -> AssetIndexDiagnosticCode {
        match self {
            Self::Json { .. } => AssetIndexDiagnosticCode::InvalidJson,
            Self::UnsupportedSchema { .. } => AssetIndexDiagnosticCode::UnsupportedSchema,
            Self::MissingField { .. } => AssetIndexDiagnosticCode::MissingField,
            Self::InvalidField { .. } => AssetIndexDiagnosticCode::InvalidField,
            Self::MissingDefault { .. } => AssetIndexDiagnosticCode::MissingDefaultBundle,
            Self::DevBuildSelection { .. } => AssetIndexDiagnosticCode::DevBuildRefused,
            Self::MismatchedBuildSelection { .. } => AssetIndexDiagnosticCode::BinaryTagMismatch,
            Self::WrongTag { .. } => AssetIndexDiagnosticCode::IndexTagMismatch,
            Self::WrongArchitecture { .. } => AssetIndexDiagnosticCode::UnsupportedHostTuple,
            Self::WrongImageKind { .. } => AssetIndexDiagnosticCode::MissingImageKind,
            Self::WrongM80Version { .. } => AssetIndexDiagnosticCode::StaleAssetIndex,
            Self::DuplicateDefault { .. } => AssetIndexDiagnosticCode::DuplicateDefaultBundle,
            Self::AssetReleaseTagMismatch { .. } => {
                AssetIndexDiagnosticCode::AssetReleaseTagMismatch
            }
            Self::TargetTupleMismatch { .. } => AssetIndexDiagnosticCode::TargetTupleMismatch,
        }
    }

    pub(super) fn diagnostic_available_tuples(&self) -> Vec<String> {
        match self {
            Self::MissingDefault { available, .. } | Self::WrongArchitecture { available, .. } => {
                available.clone()
            }
            _ => Vec::new(),
        }
    }

    pub(super) fn diagnostic_available_image_kinds(&self) -> Vec<String> {
        match self {
            Self::WrongImageKind { available, .. } => available.clone(),
            _ => Vec::new(),
        }
    }

    pub(super) fn diagnostic_available_m80_versions(&self) -> Vec<String> {
        match self {
            Self::WrongM80Version { available, .. } => available.clone(),
            _ => Vec::new(),
        }
    }

    pub(super) fn into_diagnostic(self, requested: AssetIndexRequest) -> AssetIndexDiagnostic {
        let detail = self.to_string();
        match self {
            Self::Json { .. } => requested.diagnostic(
                AssetIndexDiagnosticCode::InvalidJson,
                detail,
                Vec::new(),
                Vec::new(),
                Vec::new(),
                None,
                None,
            ),
            Self::UnsupportedSchema { .. } => requested.diagnostic(
                AssetIndexDiagnosticCode::UnsupportedSchema,
                detail,
                Vec::new(),
                Vec::new(),
                Vec::new(),
                None,
                None,
            ),
            Self::MissingField { .. } => requested.diagnostic(
                AssetIndexDiagnosticCode::MissingField,
                detail,
                Vec::new(),
                Vec::new(),
                Vec::new(),
                None,
                None,
            ),
            Self::InvalidField { .. } => requested.diagnostic(
                AssetIndexDiagnosticCode::InvalidField,
                detail,
                Vec::new(),
                Vec::new(),
                Vec::new(),
                None,
                None,
            ),
            Self::MissingDefault { available, .. } => requested.diagnostic(
                AssetIndexDiagnosticCode::MissingDefaultBundle,
                detail,
                available,
                Vec::new(),
                Vec::new(),
                None,
                Some("m80 install --bundle-url <compatible-bundle-url>".to_owned()),
            ),
            Self::DevBuildSelection { .. } => requested.diagnostic(
                AssetIndexDiagnosticCode::DevBuildRefused,
                detail,
                Vec::new(),
                Vec::new(),
                Vec::new(),
                None,
                Some("m80 install --bundle-url <compatible-bundle-url>".to_owned()),
            ),
            Self::MismatchedBuildSelection { .. } => requested.diagnostic(
                AssetIndexDiagnosticCode::BinaryTagMismatch,
                detail,
                Vec::new(),
                Vec::new(),
                Vec::new(),
                None,
                Some("curl -fsSL <matching-release-install.sh> | sudo sh".to_owned()),
            ),
            Self::WrongTag { binary, .. } => {
                let repair_url = crate::release_urls::release_install_url(&binary);
                requested.diagnostic(
                    AssetIndexDiagnosticCode::IndexTagMismatch,
                    detail,
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                    Some(repair_url.clone()),
                    Some(format!("curl -fsSL {repair_url} | sudo sh")),
                )
            }
            Self::WrongArchitecture { available, .. } => requested.diagnostic(
                AssetIndexDiagnosticCode::UnsupportedHostTuple,
                detail,
                available,
                Vec::new(),
                Vec::new(),
                None,
                Some("m80 install --bundle-url <compatible-bundle-url>".to_owned()),
            ),
            Self::WrongImageKind { available, .. } => requested.diagnostic(
                AssetIndexDiagnosticCode::MissingImageKind,
                detail,
                Vec::new(),
                available,
                Vec::new(),
                None,
                Some("m80 install --bundle-url <compatible-bundle-url>".to_owned()),
            ),
            Self::WrongM80Version { available, .. } => {
                let available_tuples = available
                    .iter()
                    .map(|version| {
                        format!(
                            "{}/{}/{}@{}",
                            requested.os, requested.arch, requested.image_kind, version
                        )
                    })
                    .collect::<Vec<_>>();
                let repair_url = available
                    .first()
                    .map(|version| crate::release_urls::release_install_url(version));
                requested.diagnostic(
                    AssetIndexDiagnosticCode::StaleAssetIndex,
                    detail,
                    available_tuples,
                    Vec::new(),
                    available,
                    repair_url.clone(),
                    repair_url.map(|url| format!("curl -fsSL {url} | sudo sh")),
                )
            }
            Self::DuplicateDefault { .. } => requested.diagnostic(
                AssetIndexDiagnosticCode::DuplicateDefaultBundle,
                detail,
                Vec::new(),
                Vec::new(),
                Vec::new(),
                None,
                None,
            ),
            Self::AssetReleaseTagMismatch { .. } => requested.diagnostic(
                AssetIndexDiagnosticCode::AssetReleaseTagMismatch,
                detail,
                Vec::new(),
                Vec::new(),
                Vec::new(),
                None,
                None,
            ),
            Self::TargetTupleMismatch { .. } => requested.diagnostic(
                AssetIndexDiagnosticCode::TargetTupleMismatch,
                detail,
                Vec::new(),
                Vec::new(),
                Vec::new(),
                None,
                None,
            ),
        }
    }
}
