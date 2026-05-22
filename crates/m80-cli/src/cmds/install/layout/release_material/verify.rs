use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use base64::Engine;
use m80_firecracker::FcError;
use serde_json::Value;

use super::support::{download_material_to_dir, release_material_error, sha256_file};
use super::verify_support::{
    expected_file_sha256, expected_subjects, material_subject_kind, parse_sha256s, read_json,
    require_commit_sha, require_nonempty, verify_public_sha256s_complete,
    verify_public_sha256s_row, verify_sidecar, verify_subject_file, verify_subject_set,
    DownloadedReleaseMaterials, PrebundleVerification, ReleaseAttestationMetadata,
    ReleaseIntegrityPredicate,
};
use super::{
    MaterialExpectation, ReleaseMaterialPlan, ReleaseVerificationSummary,
    VerifiedOfficialReleaseBundle, PUBLIC_SHA256SUMS_NAME,
};

pub(super) const RELEASE_INTEGRITY_SCHEMA_VERSION: u32 = 1;
pub(super) const RELEASE_INTEGRITY_MECHANISM: &str = "github-artifact-attestation";
pub(super) const RELEASE_ATTESTATION_SIGNER_WORKFLOW: &str =
    "moradology/m80/.github/workflows/release-artifacts.yml";
pub(super) const RELEASE_ATTESTATION_ISSUER: &str = "https://token.actions.githubusercontent.com";
pub(super) const RELEASE_ATTESTATION_KEYSET_ID: &str = "github-actions-oidc:m80-release-v1";

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
        material_paths: downloaded.paths.clone(),
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
    let asset_index_sha256 = sha256_file(downloaded.path("asset-index")?)?;

    require_equal_material(
        "release-integrity-predicate",
        "release integrity schema_version",
        integrity.schema_version,
        RELEASE_INTEGRITY_SCHEMA_VERSION,
    )?;
    require_equal_material(
        "release-integrity-predicate",
        "release integrity mechanism",
        integrity.mechanism.as_str(),
        RELEASE_INTEGRITY_MECHANISM,
    )?;
    require_equal_material(
        "release-integrity-predicate",
        "release integrity repository",
        integrity.repository.as_str(),
        crate::release_urls::release_repository().as_str(),
    )?;
    require_equal_material(
        "release-integrity-predicate",
        "release integrity release_tag",
        integrity.release_tag.as_str(),
        plan.release_tag.as_str(),
    )?;
    require_equal_material(
        "release-integrity-predicate",
        "release integrity target",
        integrity.target.as_str(),
        plan.target.as_str(),
    )?;
    require_equal_material(
        "release-integrity-predicate",
        "release integrity bundle_metadata_name",
        integrity.bundle_metadata_name.as_str(),
        plan.material("bundle-metadata")?.name.as_str(),
    )?;
    require_equal_material(
        "release-integrity-predicate",
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

    require_equal_material(
        "release-attestation-metadata",
        "release attestation schema_version",
        attestation.schema_version,
        RELEASE_INTEGRITY_SCHEMA_VERSION,
    )?;
    require_equal_material(
        "release-attestation-metadata",
        "release attestation mechanism",
        attestation.mechanism.as_str(),
        RELEASE_INTEGRITY_MECHANISM,
    )?;
    require_equal_material(
        "release-attestation-metadata",
        "release attestation repository",
        attestation.repository.as_str(),
        crate::release_urls::release_repository().as_str(),
    )?;
    require_equal_material(
        "release-attestation-metadata",
        "release attestation release_tag",
        attestation.release_tag.as_str(),
        plan.release_tag.as_str(),
    )?;
    require_equal_material(
        "release-attestation-metadata",
        "release attestation predicate_sha256",
        attestation.predicate_sha256.as_str(),
        predicate_sha256.as_str(),
    )?;
    require_equal_material(
        "release-attestation-metadata",
        "release attestation signer_identity trust-policy",
        attestation.signer_identity.as_str(),
        RELEASE_ATTESTATION_SIGNER_WORKFLOW,
    )?;
    require_equal_material(
        "release-attestation-metadata",
        "release attestation issuer trust-policy",
        attestation.issuer.as_str(),
        RELEASE_ATTESTATION_ISSUER,
    )?;
    require_equal_material(
        "release-attestation-metadata",
        "release attestation keyset_id trust-policy",
        attestation.keyset_id.as_str(),
        RELEASE_ATTESTATION_KEYSET_ID,
    )?;
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

    verify_native_attestation_bundle(plan, downloaded, &integrity, &predicate_sha256)?;

    let install_sh_sha256 = sha256_file(downloaded.path("install-script")?)?;
    Ok(PrebundleVerification {
        public_sha256s,
        integrity,
        install_sh_sha256,
        predicate_sha256,
        public_sha256s_sha256,
        asset_index_sha256,
        attestation_signer: attestation.signer_identity,
        attestation_issuer: attestation.issuer,
        attestation_keyset_id: attestation.keyset_id,
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

    let bundle_sha256 = sha256_file(downloaded.path("bundle")?)?;
    if let MaterialExpectation::BundleIdentity { sha256, size_bytes } = &bundle.expectation {
        if &bundle_sha256 != sha256 {
            return Err(release_material_error(format!(
                "release material bundle digest mismatch: {} expected_sha256={} observed_sha256={actual_sha256}",
                bundle.context(),
                sha256,
                actual_sha256 = bundle_sha256
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
        release_tag: plan.release_tag.clone(),
        repository: prebundle.integrity.repository,
        target: prebundle.integrity.target,
        bundle_asset: bundle.name.clone(),
        bundle_url: bundle.url.clone(),
        bundle_sha256,
        install_sh_sha256: prebundle.install_sh_sha256,
        predicate_sha256: prebundle.predicate_sha256,
        public_sha256s_sha256: prebundle.public_sha256s_sha256,
        asset_index_sha256: prebundle.asset_index_sha256,
        attestation_signer: prebundle.attestation_signer,
        attestation_issuer: prebundle.attestation_issuer,
        attestation_keyset_id: prebundle.attestation_keyset_id,
        source_commit: prebundle.integrity.commit_sha,
        release_integrity_schema_version: prebundle.integrity.schema_version,
    })
}

fn verify_native_attestation_bundle(
    plan: &ReleaseMaterialPlan,
    downloaded: &DownloadedReleaseMaterials,
    integrity: &ReleaseIntegrityPredicate,
    predicate_sha256: &str,
) -> Result<(), FcError> {
    let predicate_path = downloaded.path("release-integrity-predicate")?;
    let attestation_bundle_path = downloaded.path("release-attestation-bundle")?;
    let source_ref = format!("refs/tags/{}", plan.release_tag);
    let bundle = read_json_value(attestation_bundle_path, "release attestation bundle")?;
    require_value_str(
        &bundle,
        "mediaType",
        "application/vnd.dev.sigstore.bundle.v0.3+json",
        "release attestation bundle mediaType",
    )?;
    require_present(
        bundle.pointer("/verificationMaterial/certificate/rawBytes"),
        "release attestation bundle certificate rawBytes",
    )?;
    require_nonempty_array(
        bundle.pointer("/verificationMaterial/tlogEntries"),
        "release attestation bundle tlogEntries",
    )?;
    require_nonempty_array(
        bundle.pointer("/dsseEnvelope/signatures"),
        "release attestation bundle signatures",
    )?;
    require_value_str(
        &bundle,
        "/dsseEnvelope/payloadType",
        "application/vnd.in-toto+json",
        "release attestation bundle payloadType",
    )?;
    let payload_b64 = bundle
        .pointer("/dsseEnvelope/payload")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            release_material_error(
                "release attestation bundle payload missing: material_class=release-attestation-bundle"
                    .to_owned(),
            )
        })?;
    let payload = base64::engine::general_purpose::STANDARD
        .decode(payload_b64)
        .map_err(|source| {
            release_material_error(format!(
                "release attestation bundle payload base64 invalid: material_class=release-attestation-bundle source={source}"
            ))
        })?;
    let statement = serde_json::from_slice::<Value>(&payload).map_err(|source| {
        release_material_error(format!(
            "release attestation bundle statement JSON invalid: material_class=release-attestation-bundle source={source}"
        ))
    })?;

    require_value_str(
        &statement,
        "_type",
        "https://in-toto.io/Statement/v1",
        "release attestation statement type",
    )?;
    require_value_str(
        &statement,
        "predicateType",
        "https://slsa.dev/provenance/v1",
        "release attestation predicateType",
    )?;
    require_attestation_statement_names_predicate(&statement, predicate_path, predicate_sha256)?;
    require_value_str(
        &statement,
        "/predicate/buildDefinition/buildType",
        "https://actions.github.io/buildtypes/workflow/v1",
        "release attestation buildType",
    )?;
    require_value_str(
        &statement,
        "/predicate/buildDefinition/externalParameters/workflow/repository",
        "https://github.com/moradology/m80",
        "release attestation workflow repository",
    )?;
    require_value_str(
        &statement,
        "/predicate/buildDefinition/externalParameters/workflow/path",
        ".github/workflows/release-artifacts.yml",
        "release attestation workflow path",
    )?;
    require_value_str(
        &statement,
        "/predicate/buildDefinition/externalParameters/workflow/ref",
        &source_ref,
        "release attestation workflow ref",
    )?;
    require_value_str(
        &statement,
        "/predicate/buildDefinition/internalParameters/github/runner_environment",
        "github-hosted",
        "release attestation runner environment",
    )?;
    require_value_str(
        &statement,
        "/predicate/runDetails/builder/id",
        &format!(
            "https://github.com/moradology/m80/.github/workflows/release-artifacts.yml@{source_ref}"
        ),
        "release attestation builder id",
    )?;
    require_resolved_dependency(
        &statement,
        &format!("git+https://github.com/moradology/m80@{source_ref}"),
        &integrity.commit_sha,
    )
}

fn require_attestation_statement_names_predicate(
    statement: &Value,
    predicate_path: &Path,
    predicate_sha256: &str,
) -> Result<(), FcError> {
    let expected_names = [
        predicate_path.display().to_string(),
        predicate_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_owned(),
    ];
    let Some(subjects) = statement.get("subject").and_then(Value::as_array) else {
        return Err(release_material_error(
            "release attestation statement omitted subject list: material_class=release-attestation-bundle"
                .to_owned(),
        ));
    };
    for subject in subjects {
        let Some(name) = subject.get("name").and_then(Value::as_str) else {
            continue;
        };
        let Some(sha256) = subject
            .get("digest")
            .and_then(|digest| digest.get("sha256"))
            .and_then(Value::as_str)
        else {
            continue;
        };
        if expected_names.iter().any(|expected| expected == name) && sha256 == predicate_sha256 {
            return Ok(());
        }
    }
    Err(release_material_error(
        "release attestation statement omitted release-integrity predicate name/sha256 subject: material_class=release-attestation-bundle"
            .to_owned(),
    ))
}

fn require_resolved_dependency(
    statement: &Value,
    expected_uri: &str,
    expected_commit: &str,
) -> Result<(), FcError> {
    let Some(dependencies) = statement
        .pointer("/predicate/buildDefinition/resolvedDependencies")
        .and_then(Value::as_array)
    else {
        return Err(release_material_error(
            "release attestation resolvedDependencies missing: material_class=release-attestation-bundle"
                .to_owned(),
        ));
    };
    for dependency in dependencies {
        let uri = dependency.get("uri").and_then(Value::as_str);
        let commit = dependency
            .get("digest")
            .and_then(|digest| digest.get("gitCommit"))
            .and_then(Value::as_str);
        if uri == Some(expected_uri) && commit == Some(expected_commit) {
            return Ok(());
        }
    }
    Err(release_material_error(format!(
        "release attestation resolved dependency mismatch: material_class=release-attestation-bundle expected_uri={expected_uri} expected_commit={expected_commit}"
    )))
}

fn read_json_value(path: &Path, label: &str) -> Result<Value, FcError> {
    let raw = std::fs::read(path).map_err(|source| FcError::PathIo {
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

fn require_value_str(
    value: &Value,
    key_or_pointer: &str,
    expected: &str,
    label: &str,
) -> Result<(), FcError> {
    let actual = if key_or_pointer.starts_with('/') {
        value.pointer(key_or_pointer)
    } else {
        value.get(key_or_pointer)
    }
    .and_then(Value::as_str);
    if actual == Some(expected) {
        return Ok(());
    }
    Err(release_material_error(format!(
        "{label} mismatch: material_class=release-attestation-bundle expected={expected} observed={}",
        actual.unwrap_or("<missing>")
    )))
}

fn require_present(value: Option<&Value>, label: &str) -> Result<(), FcError> {
    if value.is_some() {
        return Ok(());
    }
    Err(release_material_error(format!(
        "{label} missing: material_class=release-attestation-bundle"
    )))
}

fn require_nonempty_array(value: Option<&Value>, label: &str) -> Result<(), FcError> {
    if value
        .and_then(Value::as_array)
        .is_some_and(|items| !items.is_empty())
    {
        return Ok(());
    }
    Err(release_material_error(format!(
        "{label} missing or empty: material_class=release-attestation-bundle"
    )))
}

fn require_equal_material<T>(
    material_class: &'static str,
    label: &str,
    observed: T,
    expected: T,
) -> Result<(), FcError>
where
    T: PartialEq + std::fmt::Display,
{
    if observed == expected {
        Ok(())
    } else {
        Err(release_material_error(format!(
            "{label} mismatch: material_class={material_class} expected {expected}, got {observed}"
        )))
    }
}
