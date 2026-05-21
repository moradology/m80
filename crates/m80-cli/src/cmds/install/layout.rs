use std::fs;
use std::io;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};

use m80_firecracker::{ConfigError, FcError};
use serde::Serialize;

use super::super::quickstart::profile_writer::{
    write_installed_default_profile, InstalledDefaultProfile,
};
use super::InstallPlan;
use bundle::{
    extract_bundle, list_bundle_entries, verify_entry_set, verify_extracted_tree,
    REQUIRED_BUNDLE_FILES,
};
use metadata::{
    read_bundle_metadata, rewrite_installed_metadata, set_final_modes, verify_bundle_metadata,
    verify_metadata_hashes, verify_sha256s_file, INSTALL_PROVENANCE_FILE,
};
use source::{stage_bundle_source, validate_bundle_source_url};

mod bundle;
mod metadata;
mod proof_cache;
mod reinstall;
mod release_material;
mod source;

#[cfg(test)]
pub(super) use crate::test_support::PROCESS_ENV_LOCK as INSTALL_PREFLIGHT_ENV_LOCK;

pub(super) fn preflight_attestation_verifier_for_bundle_url(
    bundle_url: &str,
) -> Result<(), FcError> {
    source::preflight_attestation_verifier_for_bundle_url(bundle_url)
}

pub(super) fn official_release_tag_from_bundle_url(
    bundle_url: &str,
) -> Result<Option<String>, FcError> {
    source::official_release_tag_from_bundle_url(bundle_url)
}

/// Summary emitted after the layout copy succeeds.
#[derive(Debug, Serialize)]
pub(super) struct LayoutInstallSummary {
    pub(super) state: &'static str,
    pub(super) release_tag: String,
    pub(super) version_dir: String,
    pub(super) files_copied: usize,
    pub(super) install_provenance: String,
    pub(super) host_binaries_manifest: String,
    pub(super) profile_path: String,
    pub(super) active_pointer: String,
    pub(super) active_pointer_flipped: bool,
    pub(super) profile_written: bool,
    pub(super) preflight_gate: &'static str,
    pub(super) finalization_order: Vec<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) release_material: Option<ReleaseMaterialInstallSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) reinstall: Option<proof_cache::ProofCacheReinstallReport>,
}

#[derive(Debug, Serialize)]
pub(super) struct ReleaseMaterialInstallSummary {
    pub(super) release_tag: String,
    pub(super) bundle_asset: String,
    pub(super) bundle_url: String,
    pub(super) bundle_sha256: String,
    pub(super) install_sh_sha256: String,
    pub(super) public_sha256s_sha256: String,
    pub(super) asset_index_sha256: String,
    pub(super) predicate_sha256: String,
    pub(super) attestation_signer: String,
    pub(super) attestation_issuer: String,
    pub(super) source_commit: String,
    pub(super) proof_cache_destination: String,
    pub(super) proof_cache_written: bool,
}

impl ReleaseMaterialInstallSummary {
    fn from_verification(
        verification: &release_material::ReleaseVerificationSummary,
        final_dir: &Path,
    ) -> Self {
        Self {
            release_tag: verification.release_tag.clone(),
            bundle_asset: verification.bundle_asset.clone(),
            bundle_url: verification.bundle_url.clone(),
            bundle_sha256: verification.bundle_sha256.clone(),
            install_sh_sha256: verification.install_sh_sha256.clone(),
            public_sha256s_sha256: verification.public_sha256s_sha256.clone(),
            asset_index_sha256: verification.asset_index_sha256.clone(),
            predicate_sha256: verification.predicate_sha256.clone(),
            attestation_signer: verification.attestation_signer.clone(),
            attestation_issuer: verification.attestation_issuer.clone(),
            source_commit: verification.source_commit.clone(),
            proof_cache_destination: release_proof_cache_destination(final_dir)
                .display()
                .to_string(),
            proof_cache_written: false,
        }
    }
}

pub(super) fn install_bundle_layout(plan: &InstallPlan) -> Result<LayoutInstallSummary, FcError> {
    let bundle_url = require_bundle_url(plan)?;
    let install_root = PathBuf::from(&plan.install_root);
    require_absolute_path("install_root", &install_root)?;
    validate_bundle_source_url(bundle_url)
        .map_err(|err| with_bundle_url_retry_context(err, bundle_url, &install_root))?;
    preflight_attestation_verifier_for_bundle_url(bundle_url)
        .map_err(|err| with_bundle_url_retry_context(err, bundle_url, &install_root))?;
    let verified_official_bundle = release_material::verify_official_release_bundle(bundle_url)
        .map_err(|err| with_bundle_url_retry_context(err, bundle_url, &install_root))?;
    let staging_dir = prepare_staging_dir(&install_root)?;
    let bundle_path = if let Some(verified_bundle) = &verified_official_bundle {
        stage_verified_bundle(verified_bundle.bundle_path(), staging_dir.path())?
    } else {
        stage_bundle_source(bundle_url, staging_dir.path())?
    };
    let entries = list_bundle_entries(&bundle_path)?;
    verify_entry_set(&entries)?;

    let extracted_dir = staging_dir.path().join("bundle");
    fs::create_dir(&extracted_dir).map_err(|source| FcError::PathIo {
        path: extracted_dir.clone(),
        source,
    })?;
    extract_bundle(&bundle_path, &extracted_dir)?;
    verify_extracted_tree(&extracted_dir)?;

    let metadata = read_bundle_metadata(&extracted_dir.join("bundle.json"))?;
    verify_bundle_metadata(&metadata)?;
    if let Some(source_tag) = plan.source.release_tag.as_deref() {
        if source_tag != metadata.release_tag {
            return Err(FcError::Config(ConfigError::InvalidValue {
                field: "bundle.release_tag",
                reason: format!(
                    "bundle release_tag mismatch: source selected {source_tag}, bundle contains {}",
                    metadata.release_tag
                ),
            }));
        }
    }
    verify_metadata_hashes(&extracted_dir, &metadata)?;
    verify_sha256s_file(&extracted_dir)?;

    let final_dir = install_root
        .join("versions")
        .join(safe_release_dir(&metadata.release_tag)?);
    rewrite_installed_metadata(&extracted_dir, &final_dir, &metadata)?;
    set_final_modes(&extracted_dir)?;
    if final_dir.exists() {
        if let Some(verified_bundle) = &verified_official_bundle {
            let verification = reinstall::verify_same_version_reinstall(
                plan,
                bundle_url,
                &install_root,
                &final_dir,
                &extracted_dir,
                &metadata.release_tag,
            )?;
            let reinstall = proof_cache::compare_existing_release_proof_cache(
                verified_bundle,
                &final_dir,
                &plan.binary_version,
                &verification.repair_command,
            )?;
            return Ok(idempotent_reinstall_summary(
                plan,
                &metadata.release_tag,
                &final_dir,
                &install_root,
                &verified_bundle.summary,
                reinstall,
            ));
        }
        return Err(FcError::Config(ConfigError::InvalidValue {
            field: "install.version_dir",
            reason: format!("version directory already exists: {}", final_dir.display()),
        }));
    }

    let proof_cache_manifest = verified_official_bundle
        .as_ref()
        .map(|verified_bundle| {
            proof_cache::write_verified_release_proof_cache(
                verified_bundle,
                &extracted_dir,
                &plan.binary_version,
            )
        })
        .transpose()?;
    let versions_dir = final_dir
        .parent()
        .expect("version directory should have versions parent");
    fs::create_dir_all(versions_dir).map_err(|source| FcError::PathIo {
        path: versions_dir.to_path_buf(),
        source,
    })?;
    fs::rename(&extracted_dir, &final_dir).map_err(|source| FcError::PathIo {
        path: final_dir.clone(),
        source,
    })?;

    let binary_config = installed_binary_config(&final_dir, &metadata);
    let host_binaries_manifest = write_install_host_binaries_manifest(&final_dir, &binary_config)?;
    let run_root = install_root.join("run");
    fs::create_dir_all(&run_root).map_err(|source| FcError::PathIo {
        path: run_root.clone(),
        source,
    })?;
    let profile_path = write_installed_default_profile(InstalledDefaultProfile {
        artifact_dir: &final_dir.join("artifacts"),
        run_root: &run_root,
        profile_dir: &install_root.join("profiles"),
        config_path: &install_root.join("config.toml"),
        binary_config,
        release_tag: Some(metadata.release_tag.clone()),
        m80_version: plan.binary_version.clone(),
        host_binaries_manifest: &host_binaries_manifest,
    })?;
    maybe_inject_interruption_after_profile()?;
    let preflight_gate = verify_preflight_gate(bundle_url)?;

    let active_pointer = PathBuf::from(&plan.active_pointer);
    flip_active_pointer(&active_pointer, &final_dir)?;

    let install_provenance = final_dir.join("artifacts").join(INSTALL_PROVENANCE_FILE);
    let release_material = verified_official_bundle.as_ref().map(|verified_bundle| {
        let mut summary =
            ReleaseMaterialInstallSummary::from_verification(&verified_bundle.summary, &final_dir);
        summary.proof_cache_written = proof_cache_manifest.is_some();
        summary
    });
    Ok(LayoutInstallSummary {
        state: "installed",
        release_tag: metadata.release_tag,
        version_dir: final_dir.display().to_string(),
        files_copied: REQUIRED_BUNDLE_FILES.len() + 1,
        install_provenance: install_provenance.display().to_string(),
        host_binaries_manifest: host_binaries_manifest.display().to_string(),
        profile_path: profile_path.display().to_string(),
        active_pointer: active_pointer.display().to_string(),
        active_pointer_flipped: true,
        profile_written: true,
        preflight_gate,
        finalization_order: finalization_order(proof_cache_manifest.is_some()),
        release_material,
        reinstall: None,
    })
}

pub(super) fn with_bundle_url_retry_context(
    err: FcError,
    bundle_url: &str,
    install_root: &Path,
) -> FcError {
    release_material::with_install_retry_context(err, bundle_url, install_root)
}

fn release_proof_cache_destination(final_dir: &Path) -> PathBuf {
    final_dir.join("artifacts").join("release-proof-cache")
}

fn idempotent_reinstall_summary(
    plan: &InstallPlan,
    release_tag: &str,
    final_dir: &Path,
    install_root: &Path,
    verification: &release_material::ReleaseVerificationSummary,
    reinstall: proof_cache::ProofCacheReinstallReport,
) -> LayoutInstallSummary {
    let host_binaries_manifest = final_dir
        .join("artifacts")
        .join("host-binaries.manifest.json");
    LayoutInstallSummary {
        state: "already_installed",
        release_tag: release_tag.to_owned(),
        version_dir: final_dir.display().to_string(),
        files_copied: 0,
        install_provenance: final_dir
            .join("artifacts")
            .join(INSTALL_PROVENANCE_FILE)
            .display()
            .to_string(),
        host_binaries_manifest: host_binaries_manifest.display().to_string(),
        profile_path: install_root
            .join("profiles/default.toml")
            .display()
            .to_string(),
        active_pointer: plan.active_pointer.clone(),
        active_pointer_flipped: false,
        profile_written: false,
        preflight_gate: "not_run_idempotent_reinstall",
        finalization_order: vec![
            "bundle_verification",
            "release_proof_cache_idempotency_check",
            "no_active_state_change",
        ],
        release_material: Some(ReleaseMaterialInstallSummary::from_verification(
            verification,
            final_dir,
        )),
        reinstall: Some(reinstall),
    }
}

#[cfg(test)]
pub(super) fn reinstall_summary_for_render_test() -> LayoutInstallSummary {
    LayoutInstallSummary {
        state: "already_installed",
        release_tag: "v0.0.0".to_owned(),
        version_dir: "/opt/m80/versions/v0.0.0".to_owned(),
        files_copied: 0,
        install_provenance: "/opt/m80/versions/v0.0.0/artifacts/install-provenance.json".to_owned(),
        host_binaries_manifest: "/opt/m80/versions/v0.0.0/artifacts/host-binaries.manifest.json"
            .to_owned(),
        profile_path: "/opt/m80/profiles/default.toml".to_owned(),
        active_pointer: "/opt/m80/active".to_owned(),
        active_pointer_flipped: false,
        profile_written: false,
        preflight_gate: "not_run_idempotent_reinstall",
        finalization_order: vec![
            "bundle_verification",
            "release_proof_cache_idempotency_check",
            "no_active_state_change",
        ],
        release_material: None,
        reinstall: Some(proof_cache::ProofCacheReinstallReport {
            status: "idempotent_same_material",
            existing_manifest_digest: "existing-digest".to_owned(),
            verified_manifest_digest: "verified-digest".to_owned(),
        }),
    }
}

fn stage_verified_bundle(bundle_path: &Path, staging_dir: &Path) -> Result<PathBuf, FcError> {
    let staged = staging_dir.join("bundle.tar.gz");
    fs::copy(bundle_path, &staged).map_err(|source| FcError::PathIo {
        path: staged.clone(),
        source,
    })?;
    Ok(staged)
}

fn require_bundle_url(plan: &InstallPlan) -> Result<&str, FcError> {
    plan.source
        .bundle_url
        .as_deref()
        .ok_or(FcError::Config(ConfigError::MissingField {
            field: "install.bundle_url",
        }))
}

fn require_absolute_path(field: &'static str, path: &Path) -> Result<(), FcError> {
    if path.is_absolute() {
        Ok(())
    } else {
        Err(FcError::Config(ConfigError::InvalidValue {
            field,
            reason: format!("{field} must be an absolute path, got {}", path.display()),
        }))
    }
}

struct StagingDir {
    path: PathBuf,
}

impl StagingDir {
    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for StagingDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn prepare_staging_dir(install_root: &Path) -> Result<StagingDir, FcError> {
    let staging_parent = install_root.join(".staging");
    fs::create_dir_all(&staging_parent).map_err(|source| FcError::PathIo {
        path: staging_parent.clone(),
        source,
    })?;
    cleanup_abandoned_staging_dirs(&staging_parent)?;
    let staging_dir = staging_parent.join(format!("layout-{}", std::process::id()));
    if staging_dir.exists() {
        fs::remove_dir_all(&staging_dir).map_err(|source| FcError::PathIo {
            path: staging_dir.clone(),
            source,
        })?;
    }
    fs::create_dir(&staging_dir).map_err(|source| FcError::PathIo {
        path: staging_dir.clone(),
        source,
    })?;
    Ok(StagingDir { path: staging_dir })
}

fn cleanup_abandoned_staging_dirs(staging_parent: &Path) -> Result<(), FcError> {
    for entry in fs::read_dir(staging_parent).map_err(|source| FcError::PathIo {
        path: staging_parent.to_path_buf(),
        source,
    })? {
        let entry = entry.map_err(|source| FcError::PathIo {
            path: staging_parent.to_path_buf(),
            source,
        })?;
        let file_name = entry.file_name();
        let Some(name) = file_name.to_str() else {
            continue;
        };
        if name.starts_with("layout-") {
            let path = entry.path();
            remove_path_if_exists(&path)?;
        }
    }
    Ok(())
}

fn installed_binary_config(
    final_dir: &Path,
    metadata: &metadata::BundleMetadata,
) -> m80_preflight::BinaryDiscoveryConfig {
    let mut config = m80_preflight::BinaryDiscoveryConfig::from_env();
    config.jailer_harden_bin = final_dir.join("bin/m80-jailer-harden");
    config.net_helper_bin = final_dir.join("bin/m80-net-helper");
    config.expected_firecracker_version = Some(metadata.expected_firecracker_version.clone());
    config
}

fn write_install_host_binaries_manifest(
    final_dir: &Path,
    binary_config: &m80_preflight::BinaryDiscoveryConfig,
) -> Result<PathBuf, FcError> {
    let path = final_dir.join("artifacts/host-binaries.manifest.json");
    let config = m80_preflight::HostBinariesManifestConfig {
        firecracker_bin: binary_config.firecracker_bin.clone(),
        firecracker_seccomp_filter: binary_config.firecracker_seccomp_filter.clone(),
        jailer_bin: binary_config.jailer_bin.clone(),
        jailer_harden_bin: binary_config.jailer_harden_bin.clone(),
        net_helper_bin: binary_config.net_helper_bin.clone(),
        m80_bin: final_dir.join("bin/m80"),
        expected_firecracker_version: binary_config.expected_firecracker_version.clone(),
    };
    m80_preflight::write_host_binaries_manifest(&config, &path)?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).map_err(|source| {
        FcError::PathIo {
            path: path.clone(),
            source,
        }
    })?;
    Ok(path)
}

fn verify_preflight_gate(bundle_url: &str) -> Result<&'static str, FcError> {
    let config = m80_preflight::HostFeaturePreflightConfig::from_env()?;
    let discovery = if use_hostless_fixture_preflight(bundle_url)? {
        m80_preflight::verify_host_substrate_fixture(
            config,
            &m80_preflight::HostSubstrateFixture::supported_root(),
        )?
    } else {
        m80_preflight::verify_host_substrate(config)?
    };
    Ok(match discovery.proof_kind {
        m80_preflight::HostSubstrateProofKind::LivePreflight => "live_preflight",
        m80_preflight::HostSubstrateProofKind::HostlessFixture => "hostless_fixture",
    })
}

fn use_hostless_fixture_preflight(bundle_url: &str) -> Result<bool, FcError> {
    #[cfg(debug_assertions)]
    {
        Ok(std::env::var_os("M80_INSTALL_HOSTLESS_FIXTURE").is_some()
            && source::is_fixture_bundle_url(bundle_url)?)
    }
    #[cfg(not(debug_assertions))]
    {
        let _ = bundle_url;
        Ok(false)
    }
}

fn flip_active_pointer(active_pointer: &Path, final_dir: &Path) -> Result<(), FcError> {
    let parent = active_pointer.parent().ok_or_else(|| {
        FcError::Config(ConfigError::InvalidValue {
            field: "active_pointer",
            reason: format!(
                "active pointer path must have a parent: {}",
                active_pointer.display()
            ),
        })
    })?;
    fs::create_dir_all(parent).map_err(|source| FcError::PathIo {
        path: parent.to_path_buf(),
        source,
    })?;
    let tmp_link = parent.join(format!(".active.tmp-{}", std::process::id()));
    remove_path_if_exists(&tmp_link)?;
    symlink(final_dir, &tmp_link).map_err(|source| FcError::PathIo {
        path: tmp_link.clone(),
        source,
    })?;
    match fs::rename(&tmp_link, active_pointer) {
        Ok(()) => Ok(()),
        Err(source) => {
            let _ = remove_path_if_exists(&tmp_link);
            Err(FcError::PathIo {
                path: active_pointer.to_path_buf(),
                source,
            })
        }
    }
}

fn remove_path_if_exists(path: &Path) -> Result<(), FcError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
            fs::remove_dir_all(path).map_err(|source| FcError::PathIo {
                path: path.to_path_buf(),
                source,
            })
        }
        Ok(_) => fs::remove_file(path).map_err(|source| FcError::PathIo {
            path: path.to_path_buf(),
            source,
        }),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(FcError::PathIo {
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn maybe_inject_interruption_after_profile() -> Result<(), FcError> {
    #[cfg(debug_assertions)]
    {
        if std::env::var_os("M80_INSTALL_INJECT_INTERRUPTION_AFTER_PROFILE").is_some() {
            return Err(FcError::Config(ConfigError::InvalidValue {
                field: "install.finalization",
                reason: "injected interruption after profile write".to_owned(),
            }));
        }
    }
    Ok(())
}

fn finalization_order(include_release_proof_cache: bool) -> Vec<&'static str> {
    let mut order = vec![
        "bundle_verification",
        "host_prerequisite_verification",
        "install_provenance",
    ];
    if include_release_proof_cache {
        order.push("release_proof_cache");
    }
    order.extend([
        "host_binaries_manifest",
        "default_profile",
        "preflight_smoke_gate",
        "active_pointer_flip",
    ]);
    order
}

fn safe_release_dir(release_tag: &str) -> Result<&str, FcError> {
    if release_tag.is_empty()
        || release_tag.contains('/')
        || release_tag.contains('\\')
        || release_tag == "."
        || release_tag == ".."
    {
        return Err(FcError::Config(ConfigError::InvalidValue {
            field: "bundle.release_tag",
            reason: format!("release tag is not a safe directory name: {release_tag:?}"),
        }));
    }
    Ok(release_tag)
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use super::super::SourceKind;
    use super::source::stage_bundle_source;
    use super::*;

    #[test]
    fn local_file_url_requires_absolute_path() {
        let tmp = tempfile::tempdir().unwrap();
        let err = stage_bundle_source("file://relative.tar.gz", tmp.path()).unwrap_err();
        assert!(
            err.to_string().contains("absolute local path"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn release_material_install_summary_initializes_proof_cache_fields() {
        let temp = tempfile::tempdir().unwrap();
        let final_dir = temp.path().join("versions/v0.0.0");
        let verification = release_material::ReleaseVerificationSummary {
            release_tag: "v0.0.0".to_owned(),
            repository: "moradology/m80".to_owned(),
            target: "linux-x86_64".to_owned(),
            bundle_asset: "m80-linux-x86_64.tar.gz".to_owned(),
            bundle_url:
                "https://github.com/moradology/m80/releases/download/v0.0.0/m80-linux-x86_64.tar.gz"
                    .to_owned(),
            bundle_sha256: "a".repeat(64),
            install_sh_sha256: "b".repeat(64),
            public_sha256s_sha256: "c".repeat(64),
            asset_index_sha256: "d".repeat(64),
            predicate_sha256: "e".repeat(64),
            attestation_signer: "moradology/m80/.github/workflows/release-artifacts.yml".to_owned(),
            attestation_issuer: "https://token.actions.githubusercontent.com".to_owned(),
            attestation_keyset_id: "github-actions-oidc:m80-release-v1".to_owned(),
            source_commit: "0123456789abcdef0123456789abcdef01234567".to_owned(),
            release_integrity_schema_version: 1,
        };

        let summary = ReleaseMaterialInstallSummary::from_verification(&verification, &final_dir);

        assert_eq!(summary.release_tag, "v0.0.0");
        assert_eq!(summary.bundle_asset, "m80-linux-x86_64.tar.gz");
        assert_eq!(summary.install_sh_sha256, "b".repeat(64));
        assert_eq!(summary.public_sha256s_sha256, "c".repeat(64));
        assert_eq!(summary.asset_index_sha256, "d".repeat(64));
        assert_eq!(
            summary.proof_cache_destination,
            final_dir
                .join("artifacts/release-proof-cache")
                .display()
                .to_string()
        );
        assert!(!summary.proof_cache_written);
        assert!(
            !final_dir.join("artifacts/release-proof-cache").exists(),
            "diagnostics reserve the proof-cache destination but do not write cache material yet"
        );
    }

    #[test]
    fn official_release_missing_attestation_verifier_fails_before_staging() {
        let _guard = INSTALL_PREFLIGHT_ENV_LOCK.lock().unwrap();
        let temp = tempfile::tempdir().unwrap();
        let install_root = temp.path().join("install-root");
        let missing_gh = temp.path().join("missing-gh");
        let _env = AttestationGhEnv::set(&missing_gh);

        let err = install_bundle_layout(&official_release_plan(&install_root)).unwrap_err();

        let message = err.to_string();
        assert!(
            message.contains("release attestation verifier missing"),
            "{message}"
        );
        assert!(
            message.contains("Install or upgrade GitHub CLI with attestation support on Linux"),
            "{message}"
        );
        assert!(
            !install_root.exists(),
            "attestation verifier failure must happen before staging creates the install root"
        );
    }

    #[test]
    fn official_release_too_old_attestation_verifier_fails_before_staging() {
        let _guard = INSTALL_PREFLIGHT_ENV_LOCK.lock().unwrap();
        let temp = tempfile::tempdir().unwrap();
        let install_root = temp.path().join("install-root");
        let fake_gh = write_fake_gh(
            temp.path(),
            "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then printf 'gh version 2.0.0\\n'; exit 0; fi\nprintf 'unknown command \"attestation\" for \"gh\"\\n' >&2\nexit 1\n",
        );
        let _env = AttestationGhEnv::set(&fake_gh);

        let err = install_bundle_layout(&official_release_plan(&install_root)).unwrap_err();

        let message = err.to_string();
        assert!(
            message.contains("release attestation verifier unsupported"),
            "{message}"
        );
        assert!(message.contains("gh version 2.0.0"), "{message}");
        assert!(
            message.contains("unknown command \"attestation\""),
            "{message}"
        );
        assert!(
            !install_root.exists(),
            "attestation verifier failure must happen before staging creates the install root"
        );
    }

    fn official_release_plan(install_root: &Path) -> InstallPlan {
        let bundle_url =
            crate::release_urls::release_asset_url("v0.0.0", "m80-linux-x86_64.tar.gz");
        InstallPlan {
            dry_run: false,
            install_root: install_root.display().to_string(),
            active_pointer: install_root.join("active").display().to_string(),
            source: super::super::SourcePlan {
                kind: SourceKind::BundleUrl,
                selector: bundle_url.clone(),
                release_tag: Some("v0.0.0".to_owned()),
                bundle_url: Some(bundle_url),
            },
            binary_version: "v0.0.0".to_owned(),
            binary_release_tag: Some("v0.0.0".to_owned()),
            version_status: "release".to_owned(),
        }
    }

    fn write_fake_gh(root: &Path, body: &str) -> PathBuf {
        let path = root.join("fake-gh");
        fs::write(&path, body).unwrap();
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&path, permissions).unwrap();
        path
    }

    struct AttestationGhEnv {
        previous: Option<OsString>,
    }

    impl AttestationGhEnv {
        fn set(path: &Path) -> Self {
            let previous = std::env::var_os("M80_RELEASE_ATTESTATION_GH");
            std::env::set_var("M80_RELEASE_ATTESTATION_GH", path);
            Self { previous }
        }
    }

    impl Drop for AttestationGhEnv {
        fn drop(&mut self) {
            match &self.previous {
                Some(value) => std::env::set_var("M80_RELEASE_ATTESTATION_GH", value),
                None => std::env::remove_var("M80_RELEASE_ATTESTATION_GH"),
            }
        }
    }
}
