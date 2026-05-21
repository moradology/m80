//! Release asset index fetching, parsing, and host tuple selection.

use std::collections::BTreeSet;

use serde::Deserialize;

use crate::release::{VersionIdentity, VersionStatus};

mod diagnostics;
mod fetch;
mod structured;

pub(crate) use structured::{AssetIndexDiagnostic, AssetIndexDiagnosticCode, AssetIndexFailure};

pub(crate) const ASSET_INDEX_SCHEMA_VERSION: u32 = 1;
const ASSET_INDEX_NAME: &str = "m80-release-assets.json";
const DEFAULT_IMAGE_KIND: &str = "minimal";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InstallerBundleSelection {
    pub(crate) bundle_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DirectBundleIndexMaterial {
    pub(crate) release_tag: String,
    pub(crate) bundle_name: String,
    pub(crate) bundle_url: String,
    pub(crate) bundle_sha256: String,
    pub(crate) bundle_size_bytes: u64,
    pub(crate) metadata_name: String,
    pub(crate) metadata_sha256: String,
    pub(crate) checksum_name: String,
    pub(crate) attestation_name: Option<String>,
    pub(crate) target: String,
    pub(crate) image_kind: String,
    pub(crate) m80_version: String,
    pub(crate) index_url: String,
    pub(crate) index_checksum_url: String,
    pub(crate) index_expected_sha256: String,
    pub(crate) index_observed_sha256: String,
}

pub(crate) fn select_release_bundle_for_install(
    release_tag: &str,
    identity: &VersionIdentity,
) -> Result<InstallerBundleSelection, AssetIndexFailure> {
    let index_url = fetch::github_release_asset_index_url(release_tag);
    select_release_bundle_for_install_at_index_url(release_tag, identity, &index_url)
}

#[cfg(test)]
pub(crate) fn select_release_bundle_for_install_from_index_url(
    release_tag: &str,
    identity: &VersionIdentity,
    index_url: &str,
) -> Result<InstallerBundleSelection, AssetIndexFailure> {
    select_release_bundle_for_install_at_index_url(release_tag, identity, index_url)
}

fn select_release_bundle_for_install_at_index_url(
    release_tag: &str,
    identity: &VersionIdentity,
    index_url: &str,
) -> Result<InstallerBundleSelection, AssetIndexFailure> {
    select_release_bundle_for_install_at_index_url_with_bounds(
        release_tag,
        identity,
        index_url,
        fetch::AssetIndexDownloadBounds::default(),
    )
}

#[cfg(test)]
pub(crate) fn select_release_bundle_for_install_from_index_url_with_download_bounds(
    release_tag: &str,
    identity: &VersionIdentity,
    index_url: &str,
    connect_timeout_seconds: u64,
    max_time_seconds: u64,
) -> Result<InstallerBundleSelection, AssetIndexFailure> {
    select_release_bundle_for_install_at_index_url_with_bounds(
        release_tag,
        identity,
        index_url,
        fetch::AssetIndexDownloadBounds::for_test(connect_timeout_seconds, max_time_seconds),
    )
}

fn select_release_bundle_for_install_at_index_url_with_bounds(
    release_tag: &str,
    identity: &VersionIdentity,
    index_url: &str,
    download_bounds: fetch::AssetIndexDownloadBounds,
) -> Result<InstallerBundleSelection, AssetIndexFailure> {
    let host = HostTuple::current();
    let requested =
        AssetIndexRequest::from_identity(release_tag, identity, host, DEFAULT_IMAGE_KIND);
    let verified = fetch::fetch_verified_asset_index(fetch::AssetIndexFetchRequest {
        index_url,
        release_tag,
        host,
        image_kind: Some(DEFAULT_IMAGE_KIND),
        download_bounds,
    })
    .map_err(|source| AssetIndexFailure::from_fetch(source, &identity.binary_version))?;
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
        .map_err(|source| AssetIndexFailure::from_selection(source, requested))?;

    Ok(InstallerBundleSelection {
        bundle_url: asset.url.clone(),
    })
}

pub(crate) fn fetch_direct_bundle_index_material(
    release_tag: &str,
    bundle_name: &str,
    bundle_url: &str,
) -> Result<DirectBundleIndexMaterial, String> {
    let index_url = fetch::github_release_asset_index_url(release_tag);
    fetch_direct_bundle_index_material_at_index_url(
        release_tag,
        bundle_name,
        bundle_url,
        &index_url,
    )
}

fn fetch_direct_bundle_index_material_at_index_url(
    release_tag: &str,
    bundle_name: &str,
    bundle_url: &str,
    index_url: &str,
) -> Result<DirectBundleIndexMaterial, String> {
    let host = HostTuple::current();
    let verified = fetch::fetch_verified_asset_index(fetch::AssetIndexFetchRequest {
        index_url,
        release_tag,
        host,
        image_kind: Some(DEFAULT_IMAGE_KIND),
        download_bounds: fetch::AssetIndexDownloadBounds::default(),
    })
    .map_err(|source| source.to_string())?;
    let asset = verified
        .index
        .direct_bundle_asset(bundle_name)
        .ok_or_else(|| {
            format!(
                "release asset index has no direct bundle row for {bundle_name}; available bundles: {}",
                verified.index.available_bundle_names().join(", ")
            )
        })?;
    if asset.url != bundle_url {
        return Err(format!(
            "release asset index URL mismatch for {bundle_name}: expected {bundle_url}, got {}",
            asset.url
        ));
    }
    let expected_checksum_name = format!("{bundle_name}.sha256");
    if asset.checksum_name != expected_checksum_name {
        return Err(format!(
            "release asset index checksum_name mismatch for {bundle_name}: expected {expected_checksum_name}, got {}",
            asset.checksum_name
        ));
    }
    Ok(DirectBundleIndexMaterial::from_verified_asset(
        asset,
        verified.index_url,
        verified.checksum_url,
        verified.expected_sha256,
        verified.observed_sha256,
    ))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AssetIndexRequest {
    os: String,
    arch: String,
    image_kind: String,
    release_tag: String,
    m80_version: String,
}

impl AssetIndexRequest {
    fn from_identity(
        release_tag: &str,
        identity: &VersionIdentity,
        host: HostTuple<'_>,
        image_kind: &str,
    ) -> Self {
        Self {
            os: host.os.to_owned(),
            arch: host.arch.to_owned(),
            image_kind: image_kind.to_owned(),
            release_tag: release_tag.to_owned(),
            m80_version: identity.binary_version.clone(),
        }
    }

    fn from_current_host(release_tag: &str, m80_version: &str) -> Self {
        let host = HostTuple::current();
        Self {
            os: host.os.to_owned(),
            arch: host.arch.to_owned(),
            image_kind: DEFAULT_IMAGE_KIND.to_owned(),
            release_tag: release_tag.to_owned(),
            m80_version: m80_version.to_owned(),
        }
    }

    fn diagnostic(
        self,
        code: AssetIndexDiagnosticCode,
        detail: String,
        available_tuples: Vec<String>,
        available_image_kinds: Vec<String>,
        available_m80_versions: Vec<String>,
        repair_url: Option<String>,
        repair_command: Option<String>,
    ) -> AssetIndexDiagnostic {
        AssetIndexDiagnostic {
            code,
            detail,
            requested_os: self.os,
            requested_arch: self.arch,
            requested_image_kind: self.image_kind,
            requested_release_tag: self.release_tag,
            requested_m80_version: self.m80_version,
            available_tuples,
            available_image_kinds,
            available_m80_versions,
            index_url: None,
            fetch_url: None,
            checksum_verification: None,
            repair_url,
            repair_command,
        }
    }
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

    fn direct_bundle_asset<'a>(&'a self, bundle_name: &str) -> Option<&'a BundleAsset> {
        self.assets.iter().find(|asset| asset.name == bundle_name)
    }

    fn available_bundle_names(&self) -> Vec<String> {
        self.assets
            .iter()
            .map(|asset| asset.name.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
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

impl DirectBundleIndexMaterial {
    fn from_verified_asset(
        asset: &BundleAsset,
        index_url: String,
        index_checksum_url: String,
        index_expected_sha256: String,
        index_observed_sha256: String,
    ) -> Self {
        Self {
            release_tag: asset.release_tag.clone(),
            bundle_name: asset.name.clone(),
            bundle_url: asset.url.clone(),
            bundle_sha256: asset.sha256.clone(),
            bundle_size_bytes: asset.size_bytes,
            metadata_name: asset.metadata_name.clone(),
            metadata_sha256: asset.metadata_sha256.clone(),
            checksum_name: asset.checksum_name.clone(),
            attestation_name: asset.attestation_name.clone(),
            target: asset.target.clone(),
            image_kind: asset.image_kind.clone(),
            m80_version: asset.m80_version.clone(),
            index_url,
            index_checksum_url,
            index_expected_sha256,
            index_observed_sha256,
        }
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
