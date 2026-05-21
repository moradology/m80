use std::collections::BTreeMap;
use std::fs;

use m80_firecracker::FcError;

use super::support::{download_material_to_dir, release_material_error, sha256_file};
use super::verify_support::{
    expected_file_sha256, expected_subjects, material_subject_kind, parse_sha256s, read_json,
    require_commit_sha, require_equal, require_nonempty, verify_public_sha256s_complete,
    verify_public_sha256s_row, verify_sidecar, verify_subject_file, verify_subject_set,
    DownloadedReleaseMaterials, PrebundleVerification, ReleaseAttestationMetadata,
    ReleaseIntegrityPredicate,
};
use super::{
    MaterialExpectation, ReleaseMaterialPlan, ReleaseVerificationSummary,
    VerifiedOfficialReleaseBundle, PUBLIC_SHA256SUMS_NAME,
};

const RELEASE_INTEGRITY_SCHEMA_VERSION: u32 = 1;
const RELEASE_INTEGRITY_MECHANISM: &str = "github-artifact-attestation";
const RELEASE_ATTESTATION_SIGNER_WORKFLOW: &str =
    "moradology/m80/.github/workflows/release-artifacts.yml";
const RELEASE_ATTESTATION_ISSUER: &str = "https://token.actions.githubusercontent.com";

pub(super) fn verify_official_bundle(
    plan: &ReleaseMaterialPlan,
) -> Result<VerifiedOfficialReleaseBundle, FcError> {
    let temp_dir = tempfile::Builder::new()
        .prefix("m80-release-material-")
        .tempdir()
        .map_err(|source| {
            release_material_error(format!(
                "release material tempdir failed: release_tag={} source={source}",
                plan.release_tag
            ))
        })?;
    let mut downloaded = DownloadedReleaseMaterials {
        paths: BTreeMap::new(),
    };

    for material in plan
        .materials
        .iter()
        .filter(|material| material.class != "bundle")
    {
        let path = download_material_to_dir(material, temp_dir.path())?;
        material.verify_download(&path)?;
        downloaded.paths.insert(material.class, path);
    }
    let prebundle = verify_prebundle_material(plan, &downloaded)?;

    let bundle = plan.material("bundle")?;
    let bundle_path = download_material_to_dir(bundle, temp_dir.path())?;
    downloaded.paths.insert(bundle.class, bundle_path.clone());
    let summary = verify_full_material(plan, &downloaded, prebundle)?;

    Ok(VerifiedOfficialReleaseBundle {
        _temp_dir: temp_dir,
        bundle_path,
        summary,
    })
}

fn verify_prebundle_material(
    plan: &ReleaseMaterialPlan,
    downloaded: &DownloadedReleaseMaterials,
) -> Result<PrebundleVerification, FcError> {
    let public_sha256s = parse_sha256s(downloaded.path("public-sha256s")?)?;
    let integrity = read_json::<ReleaseIntegrityPredicate>(
        downloaded.path("release-integrity-predicate")?,
        "release integrity predicate",
    )?;
    let attestation = read_json::<ReleaseAttestationMetadata>(
        downloaded.path("release-attestation-metadata")?,
        "release attestation metadata",
    )?;
    let predicate_sha256 = sha256_file(downloaded.path("release-integrity-predicate")?)?;
    let public_sha256s_sha256 = sha256_file(downloaded.path("public-sha256s")?)?;

    require_equal(
        "release integrity schema_version",
        integrity.schema_version,
        RELEASE_INTEGRITY_SCHEMA_VERSION,
    )?;
    require_equal(
        "release integrity mechanism",
        integrity.mechanism.as_str(),
        RELEASE_INTEGRITY_MECHANISM,
    )?;
    require_equal(
        "release integrity repository",
        integrity.repository.as_str(),
        crate::release_urls::release_repository().as_str(),
    )?;
    require_equal(
        "release integrity release_tag",
        integrity.release_tag.as_str(),
        plan.release_tag.as_str(),
    )?;
    require_equal(
        "release integrity target",
        integrity.target.as_str(),
        plan.target.as_str(),
    )?;
    require_equal(
        "release integrity bundle_metadata_name",
        integrity.bundle_metadata_name.as_str(),
        plan.material("bundle-metadata")?.name.as_str(),
    )?;
    require_equal(
        "release integrity bundle_metadata_sha256",
        integrity.bundle_metadata_sha256.as_str(),
        expected_file_sha256(plan.material("bundle-metadata")?)?,
    )?;
    require_commit_sha("release integrity commit_sha", &integrity.commit_sha)?;
    require_nonempty(
        "release integrity rust_toolchain",
        &integrity.rust_toolchain,
    )?;
    require_nonempty(
        "release integrity m80_package_version",
        &integrity.m80_package_version,
    )?;

    require_equal(
        "release attestation schema_version",
        attestation.schema_version,
        RELEASE_INTEGRITY_SCHEMA_VERSION,
    )?;
    require_equal(
        "release attestation mechanism",
        attestation.mechanism.as_str(),
        RELEASE_INTEGRITY_MECHANISM,
    )?;
    require_equal(
        "release attestation repository",
        attestation.repository.as_str(),
        crate::release_urls::release_repository().as_str(),
    )?;
    require_equal(
        "release attestation release_tag",
        attestation.release_tag.as_str(),
        plan.release_tag.as_str(),
    )?;
    require_equal(
        "release attestation predicate_sha256",
        attestation.predicate_sha256.as_str(),
        predicate_sha256.as_str(),
    )?;
    require_equal(
        "release attestation signer_identity",
        attestation.signer_identity.as_str(),
        RELEASE_ATTESTATION_SIGNER_WORKFLOW,
    )?;
    require_equal(
        "release attestation issuer",
        attestation.issuer.as_str(),
        RELEASE_ATTESTATION_ISSUER,
    )?;
    require_nonempty("release attestation keyset_id", &attestation.keyset_id)?;
    require_nonempty(
        "release attestation certificate_not_before",
        &attestation.certificate_not_before,
    )?;
    require_nonempty(
        "release attestation certificate_not_after",
        &attestation.certificate_not_after,
    )?;

    verify_sidecar(downloaded, "asset-index", "asset-index-checksum")?;
    verify_sidecar(downloaded, "bundle-metadata", "bundle-metadata-checksum")?;
    verify_sidecar(downloaded, "install-script", "install-script-checksum")?;
    verify_sidecar(
        downloaded,
        "bootstrap-selector",
        "bootstrap-selector-checksum",
    )?;
    verify_sidecar(downloaded, "release-build", "release-build-checksum")?;

    let expected_subjects = expected_subjects(plan)?;
    verify_subject_set(&integrity.subjects, &expected_subjects)?;
    verify_public_sha256s_complete(&public_sha256s, &expected_subjects)?;
    for material in plan
        .materials
        .iter()
        .filter(|material| material.class != "bundle")
    {
        if material_subject_kind(material).is_some() {
            if material.name != PUBLIC_SHA256SUMS_NAME {
                verify_public_sha256s_row(
                    &public_sha256s,
                    material,
                    downloaded.path(material.class)?,
                )?;
            }
            verify_subject_file(
                &integrity.subjects,
                material,
                downloaded.path(material.class)?,
            )?;
        }
    }

    let install_sh_sha256 = sha256_file(downloaded.path("install-script")?)?;
    Ok(PrebundleVerification {
        public_sha256s,
        integrity,
        install_sh_sha256,
        predicate_sha256,
        public_sha256s_sha256,
        attestation_signer: attestation.signer_identity,
    })
}

fn verify_full_material(
    plan: &ReleaseMaterialPlan,
    downloaded: &DownloadedReleaseMaterials,
    prebundle: PrebundleVerification,
) -> Result<ReleaseVerificationSummary, FcError> {
    verify_sidecar(downloaded, "bundle", "bundle-checksum")?;
    let bundle = plan.material("bundle")?;
    verify_public_sha256s_row(
        &prebundle.public_sha256s,
        bundle,
        downloaded.path("bundle")?,
    )?;
    verify_subject_file(
        &prebundle.integrity.subjects,
        bundle,
        downloaded.path("bundle")?,
    )?;

    if let MaterialExpectation::BundleIdentity { sha256, size_bytes } = &bundle.expectation {
        let actual_sha256 = sha256_file(downloaded.path("bundle")?)?;
        if &actual_sha256 != sha256 {
            return Err(release_material_error(format!(
                "release material bundle digest mismatch: {} expected_sha256={} observed_sha256={actual_sha256}",
                bundle.context(),
                sha256
            )));
        }
        let bundle_path = downloaded.path("bundle")?;
        let actual_size = fs::metadata(bundle_path)
            .map_err(|source| FcError::PathIo {
                path: bundle_path.to_path_buf(),
                source,
            })?
            .len();
        if &actual_size != size_bytes {
            return Err(release_material_error(format!(
                "release material bundle size mismatch: {} expected_size_bytes={} observed_size_bytes={actual_size}",
                bundle.context(),
                size_bytes
            )));
        }
    }

    Ok(ReleaseVerificationSummary {
        install_sh_sha256: prebundle.install_sh_sha256,
        predicate_sha256: prebundle.predicate_sha256,
        public_sha256s_sha256: prebundle.public_sha256s_sha256,
        attestation_signer: prebundle.attestation_signer,
    })
}
