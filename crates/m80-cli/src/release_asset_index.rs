//! Release asset index fetching, parsing, and host tuple selection.

use std::collections::BTreeSet;

use m80_firecracker::{ConfigError, FcError};
use serde::Deserialize;

use crate::release::{VersionIdentity, VersionStatus};

mod diagnostics;
mod fetch;

const ASSET_INDEX_SCHEMA_VERSION: u32 = 1;
const ASSET_INDEX_NAME: &str = "m80-release-assets.json";
const DEFAULT_IMAGE_KIND: &str = "minimal";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InstallerBundleSelection {
    pub(crate) bundle_url: String,
}

pub(crate) fn select_release_bundle_for_install(
    release_tag: &str,
    identity: &VersionIdentity,
) -> Result<InstallerBundleSelection, FcError> {
    let index_url = fetch::github_release_asset_index_url(release_tag);
    select_release_bundle_for_install_at_index_url(release_tag, identity, &index_url)
}

#[cfg(test)]
pub(crate) fn select_release_bundle_for_install_from_index_url(
    release_tag: &str,
    identity: &VersionIdentity,
    index_url: &str,
) -> Result<InstallerBundleSelection, FcError> {
    select_release_bundle_for_install_at_index_url(release_tag, identity, index_url)
}

fn select_release_bundle_for_install_at_index_url(
    release_tag: &str,
    identity: &VersionIdentity,
    index_url: &str,
) -> Result<InstallerBundleSelection, FcError> {
    let host = HostTuple::current();
    let verified = fetch::fetch_verified_asset_index(fetch::AssetIndexFetchRequest {
        index_url,
        release_tag,
        host,
        image_kind: Some(DEFAULT_IMAGE_KIND),
    })
    .map_err(asset_index_config_error)?;
    let asset = verified
        .index
        .select_default_bundle(
            BinaryRelease {
                status: identity.version_status,
                release_tag: identity.release_tag.as_deref(),
                m80_version: &identity.binary_version,
            },
            host,
            DEFAULT_IMAGE_KIND,
        )
        .map_err(asset_index_config_error)?;

    Ok(InstallerBundleSelection {
        bundle_url: asset.url.clone(),
    })
}

fn asset_index_config_error(source: impl std::fmt::Display) -> FcError {
    FcError::Config(ConfigError::InvalidValue {
        field: "release_asset_index",
        reason: source.to_string(),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BinaryRelease<'a> {
    status: VersionStatus,
    release_tag: Option<&'a str>,
    m80_version: &'a str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct HostTuple<'a> {
    os: &'a str,
    arch: &'a str,
}

impl HostTuple<'static> {
    fn current() -> Self {
        Self {
            os: std::env::consts::OS,
            arch: std::env::consts::ARCH,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReleaseAssetIndex {
    schema_version: u32,
    release_tag: String,
    assets: Vec<BundleAsset>,
}

impl ReleaseAssetIndex {
    fn parse_json(input: &str) -> Result<Self, AssetIndexError> {
        let index: Self = serde_json::from_str(input).map_err(|source| AssetIndexError::Json {
            detail: source.to_string(),
        })?;
        if index.schema_version != ASSET_INDEX_SCHEMA_VERSION {
            return Err(AssetIndexError::UnsupportedSchema {
                expected: ASSET_INDEX_SCHEMA_VERSION,
                actual: index.schema_version,
            });
        }
        if index.release_tag.is_empty() {
            return Err(AssetIndexError::MissingField {
                field: "release_tag",
            });
        }
        if index.assets.is_empty() {
            return Err(AssetIndexError::MissingDefault {
                os: HostTuple::current().os.to_owned(),
                arch: HostTuple::current().arch.to_owned(),
                image_kind: DEFAULT_IMAGE_KIND.to_owned(),
                available: Vec::new(),
            });
        }
        for asset in &index.assets {
            asset.validate(&index.release_tag)?;
        }
        Ok(index)
    }

    fn select_default_bundle<'a>(
        &'a self,
        binary: BinaryRelease<'_>,
        host: HostTuple<'_>,
        image_kind: &'_ str,
    ) -> Result<&'a BundleAsset, AssetIndexError> {
        let Some(binary_tag) = binary.release_tag else {
            return Err(AssetIndexError::DevBuildSelection {
                m80_version: binary.m80_version.to_owned(),
            });
        };
        match binary.status {
            VersionStatus::Release => {}
            VersionStatus::Dev => {
                return Err(AssetIndexError::DevBuildSelection {
                    m80_version: binary.m80_version.to_owned(),
                });
            }
            VersionStatus::Mismatch => {
                return Err(AssetIndexError::MismatchedBuildSelection {
                    release_tag: binary_tag.to_owned(),
                    m80_version: binary.m80_version.to_owned(),
                });
            }
        }
        if self.release_tag != binary_tag {
            return Err(AssetIndexError::WrongTag {
                index: self.release_tag.clone(),
                binary: binary_tag.to_owned(),
            });
        }

        let host_matches = self
            .assets
            .iter()
            .filter(|asset| asset.os == host.os && asset.arch == host.arch)
            .collect::<Vec<_>>();
        if host_matches.is_empty() {
            return Err(AssetIndexError::WrongArchitecture {
                os: host.os.to_owned(),
                arch: host.arch.to_owned(),
                available: available_tuples(&self.assets),
            });
        }

        let image_matches = host_matches
            .iter()
            .copied()
            .filter(|asset| asset.image_kind == image_kind)
            .collect::<Vec<_>>();
        if image_matches.is_empty() {
            return Err(AssetIndexError::WrongImageKind {
                os: host.os.to_owned(),
                arch: host.arch.to_owned(),
                image_kind: image_kind.to_owned(),
                available: available_image_kinds(host_matches),
            });
        }

        let release_matches = image_matches
            .iter()
            .copied()
            .filter(|asset| asset.release_tag == binary_tag)
            .collect::<Vec<_>>();
        if release_matches.is_empty() {
            return Err(AssetIndexError::WrongTag {
                index: self.release_tag.clone(),
                binary: binary_tag.to_owned(),
            });
        }

        let version_matches = release_matches
            .iter()
            .copied()
            .filter(|asset| asset.m80_version == binary.m80_version)
            .collect::<Vec<_>>();
        if version_matches.is_empty() {
            return Err(AssetIndexError::WrongM80Version {
                expected: binary.m80_version.to_owned(),
                available: release_matches
                    .iter()
                    .map(|asset| asset.m80_version.clone())
                    .collect(),
            });
        }
        if version_matches.len() > 1 {
            return Err(AssetIndexError::DuplicateDefault {
                os: host.os.to_owned(),
                arch: host.arch.to_owned(),
                image_kind: image_kind.to_owned(),
                release_tag: binary_tag.to_owned(),
            });
        }

        Ok(version_matches[0])
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct BundleAsset {
    name: String,
    url: String,
    sha256: String,
    size_bytes: u64,
    metadata_name: String,
    metadata_sha256: String,
    checksum_name: String,
    signature_name: Option<String>,
    attestation_name: Option<String>,
    target: String,
    os: String,
    arch: String,
    image_kind: String,
    release_tag: String,
    m80_version: String,
    guest_protocol_version: u32,
    manifest_schema_version: u32,
    expected_firecracker_version: String,
}

impl BundleAsset {
    fn validate(&self, index_release_tag: &str) -> Result<(), AssetIndexError> {
        for (field, value) in [
            ("name", self.name.as_str()),
            ("url", self.url.as_str()),
            ("sha256", self.sha256.as_str()),
            ("metadata_name", self.metadata_name.as_str()),
            ("metadata_sha256", self.metadata_sha256.as_str()),
            ("checksum_name", self.checksum_name.as_str()),
            ("target", self.target.as_str()),
            ("os", self.os.as_str()),
            ("arch", self.arch.as_str()),
            ("image_kind", self.image_kind.as_str()),
            ("release_tag", self.release_tag.as_str()),
            ("m80_version", self.m80_version.as_str()),
            (
                "expected_firecracker_version",
                self.expected_firecracker_version.as_str(),
            ),
        ] {
            if value.is_empty() {
                return Err(AssetIndexError::MissingField { field });
            }
        }
        validate_sha256("sha256", &self.sha256)?;
        validate_sha256("metadata_sha256", &self.metadata_sha256)?;
        validate_optional_name("signature_name", &self.signature_name)?;
        validate_optional_name("attestation_name", &self.attestation_name)?;
        validate_nonzero("size_bytes", self.size_bytes)?;
        validate_nonzero(
            "guest_protocol_version",
            u64::from(self.guest_protocol_version),
        )?;
        validate_nonzero(
            "manifest_schema_version",
            u64::from(self.manifest_schema_version),
        )?;
        if self.release_tag != index_release_tag {
            return Err(AssetIndexError::AssetReleaseTagMismatch {
                index: index_release_tag.to_owned(),
                asset: self.release_tag.clone(),
                name: self.name.clone(),
            });
        }
        let expected_target = format!("{}-{}", self.os, self.arch);
        if self.target != expected_target {
            return Err(AssetIndexError::TargetTupleMismatch {
                name: self.name.clone(),
                target: self.target.clone(),
                os: self.os.clone(),
                arch: self.arch.clone(),
            });
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum AssetIndexError {
    Json {
        detail: String,
    },
    UnsupportedSchema {
        expected: u32,
        actual: u32,
    },
    MissingField {
        field: &'static str,
    },
    InvalidField {
        field: &'static str,
        detail: &'static str,
    },
    MissingDefault {
        os: String,
        arch: String,
        image_kind: String,
        available: Vec<String>,
    },
    DevBuildSelection {
        m80_version: String,
    },
    MismatchedBuildSelection {
        release_tag: String,
        m80_version: String,
    },
    WrongTag {
        index: String,
        binary: String,
    },
    WrongArchitecture {
        os: String,
        arch: String,
        available: Vec<String>,
    },
    WrongImageKind {
        os: String,
        arch: String,
        image_kind: String,
        available: Vec<String>,
    },
    WrongM80Version {
        expected: String,
        available: Vec<String>,
    },
    DuplicateDefault {
        os: String,
        arch: String,
        image_kind: String,
        release_tag: String,
    },
    AssetReleaseTagMismatch {
        index: String,
        asset: String,
        name: String,
    },
    TargetTupleMismatch {
        name: String,
        target: String,
        os: String,
        arch: String,
    },
}

impl std::error::Error for AssetIndexError {}

fn validate_sha256(field: &'static str, value: &str) -> Result<(), AssetIndexError> {
    if value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Ok(());
    }
    Err(AssetIndexError::InvalidField {
        field,
        detail: "must be a 64-character hexadecimal sha256 digest",
    })
}

fn validate_optional_name(
    field: &'static str,
    value: &Option<String>,
) -> Result<(), AssetIndexError> {
    if matches!(value.as_deref(), Some("")) {
        return Err(AssetIndexError::MissingField { field });
    }
    Ok(())
}

fn validate_nonzero(field: &'static str, value: u64) -> Result<(), AssetIndexError> {
    if value > 0 {
        return Ok(());
    }
    Err(AssetIndexError::InvalidField {
        field,
        detail: "must be greater than zero",
    })
}

fn available_tuples(assets: &[BundleAsset]) -> Vec<String> {
    assets
        .iter()
        .map(|asset| {
            format!(
                "{}/{}/{}@{}",
                asset.os, asset.arch, asset.image_kind, asset.m80_version
            )
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn available_image_kinds(assets: Vec<&BundleAsset>) -> Vec<String> {
    assets
        .into_iter()
        .map(|asset| asset.image_kind.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

#[cfg(test)]
mod tests;
