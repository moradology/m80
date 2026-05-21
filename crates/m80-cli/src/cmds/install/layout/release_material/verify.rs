use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::Command;

use m80_firecracker::FcError;
use serde_json::Value;

use super::super::source;
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

    verify_cryptographic_attestation(plan, downloaded, &integrity, &predicate_sha256)?;

    let install_sh_sha256 = sha256_file(downloaded.path("install-script")?)?;
    Ok(PrebundleVerification {
        public_sha256s,
        integrity,
        install_sh_sha256,
        predicate_sha256,
        public_sha256s_sha256,
        attestation_signer: attestation.signer_identity,
        attestation_issuer: attestation.issuer,
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
        attestation_issuer: prebundle.attestation_issuer,
        source_commit: prebundle.integrity.commit_sha,
    })
}

fn verify_cryptographic_attestation(
    plan: &ReleaseMaterialPlan,
    downloaded: &DownloadedReleaseMaterials,
    integrity: &ReleaseIntegrityPredicate,
    predicate_sha256: &str,
) -> Result<(), FcError> {
    let gh_bin = source::release_attestation_gh_bin();
    let predicate_path = downloaded.path("release-integrity-predicate")?;
    let attestation_bundle_path = downloaded.path("release-attestation-bundle")?;
    let source_ref = format!("refs/tags/{}", plan.release_tag);
    let output = Command::new(&gh_bin)
        .arg("attestation")
        .arg("verify")
        .arg(predicate_path)
        .arg("--repo")
        .arg(crate::release_urls::release_repository())
        .arg("--bundle")
        .arg(attestation_bundle_path)
        .arg("--signer-workflow")
        .arg(RELEASE_ATTESTATION_SIGNER_WORKFLOW)
        .arg("--cert-oidc-issuer")
        .arg(RELEASE_ATTESTATION_ISSUER)
        .arg("--source-ref")
        .arg(&source_ref)
        .arg("--source-digest")
        .arg(&integrity.commit_sha)
        .arg("--deny-self-hosted-runners")
        .arg("--format")
        .arg("json")
        .output()
        .map_err(|source| {
            release_material_error(format!(
                "release attestation verifier missing: gh_bin={gh_bin} release_tag={} predicate={} attestation_bundle={} source={source}",
                plan.release_tag,
                predicate_path.display(),
                attestation_bundle_path.display()
            ))
        })?;
    if !output.status.success() {
        return Err(release_material_error(format!(
            "release trust cryptographic attestation verification failed: gh_bin={gh_bin} release_tag={} predicate={} attestation_bundle={} repo={} signer_workflow={} issuer={} source_ref={} source_digest={} status={}{}",
            plan.release_tag,
            predicate_path.display(),
            attestation_bundle_path.display(),
            crate::release_urls::release_repository(),
            RELEASE_ATTESTATION_SIGNER_WORKFLOW,
            RELEASE_ATTESTATION_ISSUER,
            source_ref,
            integrity.commit_sha,
            output.status,
            command_output_text(&output)
        )));
    }
    require_attestation_output_names_predicate(&output.stdout, predicate_path, predicate_sha256)
}

fn require_attestation_output_names_predicate(
    stdout: &[u8],
    predicate_path: &Path,
    predicate_sha256: &str,
) -> Result<(), FcError> {
    if stdout.iter().all(|byte| byte.is_ascii_whitespace()) {
        return Err(release_material_error(
            "release attestation verifier returned empty JSON".to_owned(),
        ));
    }
    let verified = serde_json::from_slice::<Value>(stdout).map_err(|source| {
        release_material_error(format!(
            "release attestation verifier returned invalid JSON: source={source}"
        ))
    })?;
    let Some(entries) = verified.as_array().filter(|entries| !entries.is_empty()) else {
        return Err(release_material_error(
            "release attestation verifier returned no attestations".to_owned(),
        ));
    };
    let expected_names = [
        predicate_path.display().to_string(),
        predicate_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_owned(),
    ];
    for entry in entries {
        let Some(subjects) = entry
            .get("verificationResult")
            .and_then(|result| result.get("statement"))
            .and_then(|statement| statement.get("subject"))
            .and_then(Value::as_array)
        else {
            continue;
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
            if expected_names.iter().any(|expected| expected == name) && sha256 == predicate_sha256
            {
                return Ok(());
            }
        }
    }
    Err(release_material_error(
        "release attestation verifier JSON omitted release-integrity predicate name/sha256 subject"
            .to_owned(),
    ))
}

fn command_output_text(output: &std::process::Output) -> String {
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let trimmed = combined.trim();
    if trimmed.is_empty() {
        String::new()
    } else {
        format!(" output={}", trimmed.replace('\n', "\\n"))
    }
}
