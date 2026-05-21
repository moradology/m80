use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use m80_firecracker::FcError;
use serde::Deserialize;

use super::support::{
    read_checksum_line, release_material_error, sha256_file, validate_release_asset_name,
};
use super::{MaterialExpectation, ReleaseMaterial, ReleaseMaterialPlan, PUBLIC_SHA256SUMS_NAME};

pub(super) struct DownloadedReleaseMaterials {
    pub(super) paths: BTreeMap<&'static str, PathBuf>,
}

impl DownloadedReleaseMaterials {
    pub(super) fn path(&self, class: &'static str) -> Result<&Path, FcError> {
        self.paths.get(class).map(PathBuf::as_path).ok_or_else(|| {
            release_material_error(format!(
                "release material was not downloaded: material_class={class}"
            ))
        })
    }
}

pub(super) struct PrebundleVerification {
    pub(super) public_sha256s: BTreeMap<String, String>,
    pub(super) integrity: ReleaseIntegrityPredicate,
    pub(super) install_sh_sha256: String,
    pub(super) predicate_sha256: String,
    pub(super) public_sha256s_sha256: String,
    pub(super) asset_index_sha256: String,
    pub(super) attestation_signer: String,
    pub(super) attestation_issuer: String,
    pub(super) attestation_keyset_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ReleaseIntegrityPredicate {
    pub(super) schema_version: u32,
    pub(super) mechanism: String,
    pub(super) repository: String,
    pub(super) release_tag: String,
    pub(super) commit_sha: String,
    pub(super) target: String,
    pub(super) rust_toolchain: String,
    pub(super) m80_package_version: String,
    pub(super) bundle_metadata_name: String,
    pub(super) bundle_metadata_sha256: String,
    pub(super) subjects: Vec<ReleaseIntegritySubject>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ReleaseIntegritySubject {
    pub(super) name: String,
    kind: String,
    sha256: String,
    size_bytes: u64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ReleaseAttestationMetadata {
    pub(super) schema_version: u32,
    pub(super) mechanism: String,
    pub(super) repository: String,
    pub(super) release_tag: String,
    pub(super) predicate_sha256: String,
    pub(super) signer_identity: String,
    pub(super) issuer: String,
    pub(super) keyset_id: String,
    pub(super) certificate_not_before: String,
    pub(super) certificate_not_after: String,
}

pub(super) fn read_json<T: for<'de> Deserialize<'de>>(
    path: &Path,
    label: &str,
) -> Result<T, FcError> {
    let raw = fs::read(path).map_err(|source| FcError::PathIo {
        path: path.to_path_buf(),
        source,
    })?;
    serde_json::from_slice(&raw).map_err(|source| {
        release_material_error(format!(
            "{label} JSON invalid: path={} source={source}",
            path.display()
        ))
    })
}

pub(super) fn parse_sha256s(path: &Path) -> Result<BTreeMap<String, String>, FcError> {
    let text = fs::read_to_string(path).map_err(|source| FcError::PathIo {
        path: path.to_path_buf(),
        source,
    })?;
    let mut rows = BTreeMap::new();
    for (line_no, line) in text.lines().enumerate() {
        let parts = line.split_whitespace().collect::<Vec<_>>();
        if parts.len() != 2 {
            return Err(release_material_error(format!(
                "release material public SHA256SUMS row shape invalid: path={} line={}",
                path.display(),
                line_no + 1
            )));
        }
        let digest = parts[0].to_ascii_lowercase();
        require_sha256("release material public SHA256SUMS digest", &digest)?;
        validate_release_asset_name(parts[1])?;
        if rows.insert(parts[1].to_owned(), digest).is_some() {
            return Err(release_material_error(format!(
                "release material public SHA256SUMS duplicate asset: asset={}",
                parts[1]
            )));
        }
    }
    if rows.is_empty() {
        return Err(release_material_error(
            "release material public SHA256SUMS is empty".to_owned(),
        ));
    }
    Ok(rows)
}

pub(super) fn expected_subjects(
    plan: &ReleaseMaterialPlan,
) -> Result<BTreeMap<String, &'static str>, FcError> {
    let mut subjects = BTreeMap::new();
    for material in &plan.materials {
        if let Some(kind) = material_subject_kind(material) {
            if subjects.insert(material.name.clone(), kind).is_some() {
                return Err(release_material_error(format!(
                    "release material duplicate subject name: material_name={}",
                    material.name
                )));
            }
        }
    }
    Ok(subjects)
}

pub(super) fn material_subject_kind(material: &ReleaseMaterial) -> Option<&'static str> {
    match material.class {
        "bundle" => Some("release-bundle"),
        "bundle-checksum"
        | "bundle-metadata-checksum"
        | "asset-index-checksum"
        | "install-script-checksum"
        | "bootstrap-selector-checksum"
        | "release-build-checksum" => Some("checksum-sidecar"),
        "bundle-metadata" => Some("bundle-metadata"),
        "asset-index" => Some("asset-index"),
        "install-script" => Some("installer"),
        "bootstrap-selector" => Some("bootstrap-selector"),
        "release-build" => Some("build-manifest"),
        "public-sha256s" => Some("checksum-manifest"),
        _ => None,
    }
}

pub(super) fn verify_subject_set(
    subjects: &[ReleaseIntegritySubject],
    expected: &BTreeMap<String, &'static str>,
) -> Result<(), FcError> {
    let mut seen = BTreeMap::new();
    for subject in subjects {
        validate_release_asset_name(&subject.name)?;
        require_nonempty("release integrity subject kind", &subject.kind)?;
        require_sha256("release integrity subject sha256", &subject.sha256)?;
        if subject.size_bytes == 0 {
            return Err(release_material_error(format!(
                "release integrity subject size_bytes must be nonzero: subject={}",
                subject.name
            )));
        }
        let Some(expected_kind) = expected.get(&subject.name) else {
            return Err(release_material_error(format!(
                "release integrity unexpected subject: subject={}",
                subject.name
            )));
        };
        require_equal(
            "release integrity subject kind",
            subject.kind.as_str(),
            *expected_kind,
        )?;
        if seen.insert(subject.name.clone(), subject).is_some() {
            return Err(release_material_error(format!(
                "release integrity duplicate subject: subject={}",
                subject.name
            )));
        }
    }
    let actual_names = seen.keys().cloned().collect::<BTreeSet<_>>();
    let expected_names = expected.keys().cloned().collect::<BTreeSet<_>>();
    if actual_names != expected_names {
        return Err(release_material_error(format!(
            "release integrity subject set mismatch: missing={} extra={}",
            set_difference(&expected_names, &actual_names).join(","),
            set_difference(&actual_names, &expected_names).join(",")
        )));
    }
    Ok(())
}

pub(super) fn verify_public_sha256s_complete(
    rows: &BTreeMap<String, String>,
    expected_subjects: &BTreeMap<String, &'static str>,
) -> Result<(), FcError> {
    let actual = rows.keys().cloned().collect::<BTreeSet<_>>();
    let expected = expected_subjects
        .keys()
        .filter(|name| name.as_str() != PUBLIC_SHA256SUMS_NAME)
        .cloned()
        .collect::<BTreeSet<_>>();
    if actual != expected {
        return Err(release_material_error(format!(
            "release material public SHA256SUMS set mismatch: missing={} extra={}",
            set_difference(&expected, &actual).join(","),
            set_difference(&actual, &expected).join(",")
        )));
    }
    Ok(())
}

pub(super) fn verify_subject_file(
    subjects: &[ReleaseIntegritySubject],
    material: &ReleaseMaterial,
    path: &Path,
) -> Result<(), FcError> {
    let subject = subjects
        .iter()
        .find(|subject| subject.name == material.name)
        .ok_or_else(|| {
            release_material_error(format!(
                "release integrity subject missing for material: {}",
                material.context()
            ))
        })?;
    let actual_sha256 = sha256_file(path)?;
    if subject.sha256 != actual_sha256 {
        return Err(release_material_error(format!(
            "release integrity sha256 mismatch: {} expected_sha256={} observed_sha256={actual_sha256}",
            material.context(),
            subject.sha256
        )));
    }
    let actual_size = fs::metadata(path)
        .map_err(|source| FcError::PathIo {
            path: path.to_path_buf(),
            source,
        })?
        .len();
    if subject.size_bytes != actual_size {
        return Err(release_material_error(format!(
            "release integrity size mismatch: {} expected_size_bytes={} observed_size_bytes={actual_size}",
            material.context(),
            subject.size_bytes
        )));
    }
    Ok(())
}

pub(super) fn verify_public_sha256s_row(
    rows: &BTreeMap<String, String>,
    material: &ReleaseMaterial,
    path: &Path,
) -> Result<(), FcError> {
    let Some(expected_sha256) = rows.get(&material.name) else {
        return Err(release_material_error(format!(
            "release material public SHA256SUMS missing row: {}",
            material.context()
        )));
    };
    let observed_sha256 = sha256_file(path)?;
    if expected_sha256 != &observed_sha256 {
        return Err(release_material_error(format!(
            "release material public SHA256SUMS mismatch: {} expected_sha256={} observed_sha256={observed_sha256}",
            material.context(),
            expected_sha256
        )));
    }
    Ok(())
}

pub(super) fn verify_sidecar(
    downloaded: &DownloadedReleaseMaterials,
    asset_class: &'static str,
    sidecar_class: &'static str,
) -> Result<(), FcError> {
    let asset_path = downloaded.path(asset_class)?;
    let sidecar_path = downloaded.path(sidecar_class)?;
    let (expected_sha256, expected_name) = read_checksum_line(sidecar_path)?;
    let observed_sha256 = sha256_file(asset_path)?;
    if expected_sha256 != observed_sha256 {
        return Err(release_material_error(format!(
            "release material sidecar digest mismatch: material_class={asset_class} sidecar_class={sidecar_class} expected_sha256={expected_sha256} observed_sha256={observed_sha256}"
        )));
    }
    if let Some(expected_name) = expected_name {
        let Some(asset_name) = asset_path.file_name().and_then(|name| name.to_str()) else {
            return Err(release_material_error(format!(
                "release material sidecar asset path has no file name: path={}",
                asset_path.display()
            )));
        };
        if expected_name != asset_name {
            return Err(release_material_error(format!(
                "release material sidecar target mismatch: material_class={asset_class} sidecar_class={sidecar_class} expected_name={expected_name} observed_name={asset_name}"
            )));
        }
    }
    Ok(())
}

pub(super) fn expected_file_sha256(material: &ReleaseMaterial) -> Result<&str, FcError> {
    match &material.expectation {
        MaterialExpectation::FileSha256(value) => Ok(value),
        other => Err(release_material_error(format!(
            "release material expected a file sha256 expectation: material_class={} expectation={}",
            material.class,
            other.describe()
        ))),
    }
}

pub(super) fn require_equal<T>(label: &str, observed: T, expected: T) -> Result<(), FcError>
where
    T: PartialEq + std::fmt::Display,
{
    if observed == expected {
        Ok(())
    } else {
        Err(release_material_error(format!(
            "{label} mismatch: expected {expected}, got {observed}"
        )))
    }
}

pub(super) fn require_nonempty(label: &str, value: &str) -> Result<(), FcError> {
    if value.is_empty() {
        Err(release_material_error(format!("{label} missing")))
    } else {
        Ok(())
    }
}

fn require_sha256(label: &str, value: &str) -> Result<(), FcError> {
    if value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(release_material_error(format!(
            "{label} must be a 64-hex sha256 digest"
        )))
    }
}

pub(super) fn require_commit_sha(label: &str, value: &str) -> Result<(), FcError> {
    if value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(release_material_error(format!(
            "{label} must be a 40-hex commit digest"
        )))
    }
}

fn set_difference(left: &BTreeSet<String>, right: &BTreeSet<String>) -> Vec<String> {
    left.difference(right).cloned().collect()
}
