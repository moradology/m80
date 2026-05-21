use std::path::{Path, PathBuf};

use m80_firecracker::FcError;
use tempfile::TempDir;

use super::source;
use support::{
    read_checksum_line, release_asset_url, release_material_error, sha256_file,
    validate_release_asset_name,
};

const CONNECT_TIMEOUT_SECONDS: &str = "10";
const MAX_TIME_SECONDS: &str = "120";

const ASSET_INDEX_NAME: &str = "m80-release-assets.json";
const BOOTSTRAP_SELECTOR_NAME: &str = "m80-bootstrap-selector.tsv";
const INSTALL_SCRIPT_NAME: &str = "install.sh";
const PUBLIC_SHA256SUMS_NAME: &str = "SHA256SUMS";
const RELEASE_ATTESTATION_BUNDLE_NAME: &str = "m80-release-integrity.attestation.jsonl";
const RELEASE_ATTESTATION_METADATA_NAME: &str = "m80-release-attestation.json";
const RELEASE_BUILD_NAME: &str = "m80-release-build.json";
const RELEASE_INTEGRITY_NAME: &str = "m80-release-integrity.json";

mod support;
mod verify;
mod verify_support;

pub(super) fn verify_official_release_bundle(
    bundle_url: &str,
) -> Result<Option<VerifiedOfficialReleaseBundle>, FcError> {
    let Some(bundle) = source::official_release_bundle_from_url(bundle_url)? else {
        return Ok(None);
    };
    let material = crate::release_asset_index::fetch_direct_bundle_index_material(
        &bundle.release_tag,
        &bundle.bundle_name,
        &bundle.bundle_url,
    )
    .map_err(|reason| {
        release_material_error(format!(
            "release material plan failed: release_tag={} bundle={} reason={reason}",
            bundle.release_tag, bundle.bundle_name
        ))
    })?;
    let plan = ReleaseMaterialPlan::from_index_material(material)?;
    let verified = plan.verify_official_bundle()?;
    eprintln!(
        "m80 install: verified release material release_tag={} material_classes={} identity={} install_sh_sha256={} predicate_sha256={} public_sha256s_sha256={} attestation_signer={}",
        plan.release_tag,
        plan.material_classes().join(","),
        plan.identity,
        verified.summary.install_sh_sha256,
        verified.summary.predicate_sha256,
        verified.summary.public_sha256s_sha256,
        verified.summary.attestation_signer
    );
    Ok(Some(verified))
}

#[derive(Debug)]
pub(super) struct VerifiedOfficialReleaseBundle {
    pub(super) _temp_dir: TempDir,
    pub(super) bundle_path: PathBuf,
    pub(super) summary: ReleaseVerificationSummary,
}

impl VerifiedOfficialReleaseBundle {
    pub(super) fn bundle_path(&self) -> &Path {
        &self.bundle_path
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ReleaseVerificationSummary {
    pub(super) install_sh_sha256: String,
    pub(super) predicate_sha256: String,
    pub(super) public_sha256s_sha256: String,
    pub(super) attestation_signer: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReleaseMaterialPlan {
    release_tag: String,
    target: String,
    identity: String,
    materials: Vec<ReleaseMaterial>,
}

impl ReleaseMaterialPlan {
    fn from_index_material(
        material: crate::release_asset_index::DirectBundleIndexMaterial,
    ) -> Result<Self, FcError> {
        let Some(attestation_name) = material.attestation_name.clone() else {
            return Err(release_material_error(format!(
                "release material plan failed: release_tag={} bundle={} field=attestation_name reason=missing",
                material.release_tag, material.bundle_name
            )));
        };
        if attestation_name != RELEASE_ATTESTATION_BUNDLE_NAME {
            return Err(release_material_error(format!(
                "release material plan failed: release_tag={} bundle={} field=attestation_name expected={} got={}",
                material.release_tag,
                material.bundle_name,
                RELEASE_ATTESTATION_BUNDLE_NAME,
                attestation_name
            )));
        }

        let metadata_checksum_name = format!("{}.sha256", material.metadata_name);
        let asset_index_checksum_name = format!("{ASSET_INDEX_NAME}.sha256");
        let install_checksum_name = format!("{INSTALL_SCRIPT_NAME}.sha256");
        let bootstrap_selector_checksum_name = format!("{BOOTSTRAP_SELECTOR_NAME}.sha256");
        let release_build_checksum_name = format!("{RELEASE_BUILD_NAME}.sha256");
        let names = [
            material.bundle_name.as_str(),
            material.checksum_name.as_str(),
            material.metadata_name.as_str(),
            metadata_checksum_name.as_str(),
            ASSET_INDEX_NAME,
            asset_index_checksum_name.as_str(),
            INSTALL_SCRIPT_NAME,
            install_checksum_name.as_str(),
            BOOTSTRAP_SELECTOR_NAME,
            bootstrap_selector_checksum_name.as_str(),
            RELEASE_BUILD_NAME,
            release_build_checksum_name.as_str(),
            RELEASE_INTEGRITY_NAME,
            RELEASE_ATTESTATION_BUNDLE_NAME,
            RELEASE_ATTESTATION_METADATA_NAME,
            PUBLIC_SHA256SUMS_NAME,
        ];
        for name in names {
            validate_release_asset_name(name)?;
        }

        let identity = format!(
            "bundle={} target={} image_kind={} m80_version={} bundle_sha256={} bundle_size_bytes={} metadata={} metadata_sha256={} index_sha256={} index_observed_sha256={}",
            material.bundle_name,
            material.target,
            material.image_kind,
            material.m80_version,
            material.bundle_sha256,
            material.bundle_size_bytes,
            material.metadata_name,
            material.metadata_sha256,
            material.index_expected_sha256,
            material.index_observed_sha256
        );
        let release_tag = material.release_tag.clone();
        let target = material.target.clone();
        let materials = vec![
            ReleaseMaterial::listed(
                "bundle",
                material.bundle_name.clone(),
                material.bundle_url.clone(),
                MaterialExpectation::BundleIdentity {
                    sha256: material.bundle_sha256.clone(),
                    size_bytes: material.bundle_size_bytes,
                },
            ),
            ReleaseMaterial::probed(
                "bundle-checksum",
                material.checksum_name.clone(),
                release_asset_url(&release_tag, &material.checksum_name),
                MaterialExpectation::ChecksumLine {
                    sha256: material.bundle_sha256,
                    name: material.bundle_name,
                },
            ),
            ReleaseMaterial::probed(
                "bundle-metadata",
                material.metadata_name.clone(),
                release_asset_url(&release_tag, &material.metadata_name),
                MaterialExpectation::FileSha256(material.metadata_sha256.clone()),
            ),
            ReleaseMaterial::probed(
                "bundle-metadata-checksum",
                metadata_checksum_name.clone(),
                release_asset_url(&release_tag, &metadata_checksum_name),
                MaterialExpectation::ChecksumLine {
                    sha256: material.metadata_sha256,
                    name: material.metadata_name,
                },
            ),
            ReleaseMaterial::listed(
                "asset-index",
                ASSET_INDEX_NAME.to_owned(),
                material.index_url,
                MaterialExpectation::VerifiedIndexSha256 {
                    expected: material.index_expected_sha256.clone(),
                    observed: material.index_observed_sha256,
                },
            ),
            ReleaseMaterial::listed(
                "asset-index-checksum",
                asset_index_checksum_name,
                material.index_checksum_url,
                MaterialExpectation::ChecksumLine {
                    sha256: material.index_expected_sha256,
                    name: ASSET_INDEX_NAME.to_owned(),
                },
            ),
            ReleaseMaterial::probed(
                "install-script",
                INSTALL_SCRIPT_NAME.to_owned(),
                release_asset_url(&release_tag, INSTALL_SCRIPT_NAME),
                MaterialExpectation::Exists,
            ),
            ReleaseMaterial::probed(
                "install-script-checksum",
                install_checksum_name.clone(),
                release_asset_url(&release_tag, &install_checksum_name),
                MaterialExpectation::Exists,
            ),
            ReleaseMaterial::probed(
                "bootstrap-selector",
                BOOTSTRAP_SELECTOR_NAME.to_owned(),
                release_asset_url(&release_tag, BOOTSTRAP_SELECTOR_NAME),
                MaterialExpectation::Exists,
            ),
            ReleaseMaterial::probed(
                "bootstrap-selector-checksum",
                bootstrap_selector_checksum_name.clone(),
                release_asset_url(&release_tag, &bootstrap_selector_checksum_name),
                MaterialExpectation::Exists,
            ),
            ReleaseMaterial::probed(
                "release-build",
                RELEASE_BUILD_NAME.to_owned(),
                release_asset_url(&release_tag, RELEASE_BUILD_NAME),
                MaterialExpectation::Exists,
            ),
            ReleaseMaterial::probed(
                "release-build-checksum",
                release_build_checksum_name.clone(),
                release_asset_url(&release_tag, &release_build_checksum_name),
                MaterialExpectation::Exists,
            ),
            ReleaseMaterial::probed(
                "release-integrity-predicate",
                RELEASE_INTEGRITY_NAME.to_owned(),
                release_asset_url(&release_tag, RELEASE_INTEGRITY_NAME),
                MaterialExpectation::Exists,
            ),
            ReleaseMaterial::probed(
                "release-attestation-bundle",
                RELEASE_ATTESTATION_BUNDLE_NAME.to_owned(),
                release_asset_url(&release_tag, RELEASE_ATTESTATION_BUNDLE_NAME),
                MaterialExpectation::Exists,
            ),
            ReleaseMaterial::probed(
                "release-attestation-metadata",
                RELEASE_ATTESTATION_METADATA_NAME.to_owned(),
                release_asset_url(&release_tag, RELEASE_ATTESTATION_METADATA_NAME),
                MaterialExpectation::Exists,
            ),
            ReleaseMaterial::probed(
                "public-sha256s",
                PUBLIC_SHA256SUMS_NAME.to_owned(),
                release_asset_url(&release_tag, PUBLIC_SHA256SUMS_NAME),
                MaterialExpectation::Exists,
            ),
        ];

        Ok(Self {
            release_tag,
            target,
            identity,
            materials,
        })
    }

    fn verify_official_bundle(&self) -> Result<VerifiedOfficialReleaseBundle, FcError> {
        verify::verify_official_bundle(self)
    }

    fn material(&self, class: &'static str) -> Result<&ReleaseMaterial, FcError> {
        self.materials
            .iter()
            .find(|material| material.class == class)
            .ok_or_else(|| {
                release_material_error(format!("release material class missing: {class}"))
            })
    }

    fn material_classes(&self) -> Vec<&'static str> {
        self.materials
            .iter()
            .map(|material| material.class)
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReleaseMaterial {
    class: &'static str,
    name: String,
    url: String,
    expectation: MaterialExpectation,
    probe: bool,
}

impl ReleaseMaterial {
    fn listed(
        class: &'static str,
        name: String,
        url: String,
        expectation: MaterialExpectation,
    ) -> Self {
        Self {
            class,
            name,
            url,
            expectation,
            probe: false,
        }
    }

    fn probed(
        class: &'static str,
        name: String,
        url: String,
        expectation: MaterialExpectation,
    ) -> Self {
        Self {
            class,
            name,
            url,
            expectation,
            probe: true,
        }
    }

    fn context(&self) -> String {
        format!(
            "release_tag={} material_class={} material_name={} url={} expectation={}",
            release_tag_from_material_url(&self.url).unwrap_or("unknown"),
            self.class,
            self.name,
            self.url,
            self.expectation.describe()
        )
    }

    fn verify_download(&self, path: &Path) -> Result<(), FcError> {
        match &self.expectation {
            MaterialExpectation::Exists
            | MaterialExpectation::BundleIdentity { .. }
            | MaterialExpectation::VerifiedIndexSha256 { .. } => Ok(()),
            MaterialExpectation::FileSha256(expected) => {
                let observed = sha256_file(path)?;
                if observed == *expected {
                    Ok(())
                } else {
                    Err(release_material_error(format!(
                        "release material digest mismatch: {} expected_sha256={} observed_sha256={observed}",
                        self.context(),
                        expected
                    )))
                }
            }
            MaterialExpectation::ChecksumLine { sha256, name } => {
                let (observed_sha256, observed_name) = read_checksum_line(path)?;
                if observed_sha256 != *sha256 {
                    return Err(release_material_error(format!(
                        "release material checksum mismatch: {} expected_sha256={} observed_sha256={observed_sha256}",
                        self.context(),
                        sha256
                    )));
                }
                if let Some(observed_name) = observed_name {
                    if observed_name != *name {
                        return Err(release_material_error(format!(
                            "release material checksum name mismatch: {} expected_name={} observed_name={observed_name}",
                            self.context(),
                            name
                        )));
                    }
                }
                Ok(())
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum MaterialExpectation {
    Exists,
    FileSha256(String),
    ChecksumLine { sha256: String, name: String },
    BundleIdentity { sha256: String, size_bytes: u64 },
    VerifiedIndexSha256 { expected: String, observed: String },
}

impl MaterialExpectation {
    fn describe(&self) -> String {
        match self {
            Self::Exists => "exists".to_owned(),
            Self::FileSha256(sha256) => format!("file_sha256={sha256}"),
            Self::ChecksumLine { sha256, name } => {
                format!("checksum_line_sha256={sha256} checksum_line_name={name}")
            }
            Self::BundleIdentity { sha256, size_bytes } => {
                format!("bundle_sha256={sha256} size_bytes={size_bytes}")
            }
            Self::VerifiedIndexSha256 { expected, observed } => {
                format!("index_expected_sha256={expected} index_observed_sha256={observed}")
            }
        }
    }
}

fn release_tag_from_material_url(url: &str) -> Option<&str> {
    let prefix = format!(
        "https://github.com{}",
        crate::release_urls::release_download_path_prefix()
    );
    let rest = url.strip_prefix(&prefix)?;
    let (tag, _) = rest.split_once('/')?;
    if tag.is_empty() {
        None
    } else {
        Some(tag)
    }
}

#[cfg(test)]
mod test_env;
#[cfg(test)]
mod test_fixture;
#[cfg(test)]
mod tests;
