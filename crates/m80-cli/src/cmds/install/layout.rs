use std::fs;
use std::io;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use m80_firecracker::{ConfigError, FcError};
use serde::Serialize;

use crate::args::InstallSmokeGateArg;

use super::super::quickstart::profile_writer::{
    write_installed_default_profile, InstalledDefaultProfile, InstalledProfileTransaction,
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
mod lock;
mod metadata;
mod proof_cache;
mod reinstall;
mod release_material;
mod source;

const DEFAULT_INSTALL_ROOT: &str = "/opt/m80";
const DEFAULT_CONFIG_PATH: &str = "/etc/m80/config.toml";
const DEFAULT_PROFILE_DIR: &str = "/etc/m80/profiles";

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
    pub(super) install_root: String,
    pub(super) active_version_dir: String,
    pub(super) version_dir: String,
    pub(super) bundle_url: String,
    pub(super) installed_m80_path: String,
    pub(super) installed_m80_version: String,
    pub(super) active_bundle_path: String,
    pub(super) files_copied: usize,
    pub(super) install_provenance: String,
    pub(super) host_binaries_manifest: String,
    pub(super) default_profile: String,
    pub(super) profile_path: String,
    pub(super) active_pointer: String,
    pub(super) active_pointer_flipped: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) previous_active_version_dir: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) previous_active_release_tag: Option<String>,
    pub(super) profile_written: bool,
    pub(super) smoke_gate: &'static str,
    pub(super) host_prerequisite_status: String,
    pub(super) preflight_gate: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) run_smoke_command: Option<Vec<String>>,
    pub(super) next_command: String,
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
    reject_symlinked_install_root(&install_root)?;
    validate_existing_active_pointer(&install_root, &PathBuf::from(&plan.active_pointer))?;
    validate_bundle_source_url(bundle_url)
        .map_err(|err| with_bundle_url_retry_context(err, bundle_url, &install_root))?;
    preflight_attestation_verifier_for_bundle_url(bundle_url)
        .map_err(|err| with_bundle_url_retry_context(err, bundle_url, &install_root))?;
    let verified_official_bundle = release_material::verify_official_release_bundle(bundle_url)
        .map_err(|err| with_bundle_url_retry_context(err, bundle_url, &install_root))?;
    let _install_lock = lock::acquire_install_state_lock(plan, &install_root)?;
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
    let selector_paths = install_selector_paths(&install_root);
    let default_profile = selector_paths.default_profile_path();
    let mut selector_transaction =
        InstalledProfileTransaction::capture(&default_profile, &selector_paths.config_path)?;
    let profile_path = write_installed_default_profile(InstalledDefaultProfile {
        artifact_dir: &final_dir.join("artifacts"),
        run_root: &run_root,
        profile_dir: &selector_paths.profile_dir,
        config_path: &selector_paths.config_path,
        binary_config,
        release_tag: Some(metadata.release_tag.clone()),
        m80_version: plan.binary_version.clone(),
        host_binaries_manifest: &host_binaries_manifest,
        adopt_existing_config: plan.adopt_existing_config,
        adoption_command: adoption_command(bundle_url, &install_root),
    })?;
    let finalization = (|| {
        maybe_inject_interruption_after_profile()?;
        let smoke_gate = verify_smoke_gate(plan, bundle_url, &final_dir, &selector_paths)?;
        let handoff = install_path_handoff(&final_dir, Path::new(&plan.bin_dir))?;
        Ok((smoke_gate, handoff))
    })();
    let (smoke_gate, handoff) = match finalization {
        Ok(finalization) => finalization,
        Err(err) => {
            selector_transaction.rollback();
            return Err(err);
        }
    };

    let active_pointer = PathBuf::from(&plan.active_pointer);
    let previous_active = previous_active_candidate(&active_pointer)?;
    if let Err(err) = flip_active_pointer(&active_pointer, &final_dir) {
        selector_transaction.rollback();
        return Err(err);
    }

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
        install_root: install_root.display().to_string(),
        active_version_dir: final_dir.display().to_string(),
        version_dir: final_dir.display().to_string(),
        bundle_url: bundle_url.to_owned(),
        installed_m80_path: handoff.installed_m80_path,
        installed_m80_version: handoff.installed_m80_version,
        active_bundle_path: final_dir.display().to_string(),
        files_copied: REQUIRED_BUNDLE_FILES.len() + 1,
        install_provenance: install_provenance.display().to_string(),
        host_binaries_manifest: host_binaries_manifest.display().to_string(),
        default_profile: default_profile.display().to_string(),
        profile_path: profile_path.display().to_string(),
        active_pointer: active_pointer.display().to_string(),
        active_pointer_flipped: true,
        previous_active_version_dir: previous_active
            .as_ref()
            .map(|candidate| candidate.version_dir.display().to_string()),
        previous_active_release_tag: previous_active.map(|candidate| candidate.release_tag),
        profile_written: true,
        smoke_gate: smoke_gate.selected_gate,
        host_prerequisite_status: host_prerequisite_status(smoke_gate.preflight_gate),
        preflight_gate: smoke_gate.preflight_gate,
        run_smoke_command: smoke_gate.run_smoke_command,
        next_command: "m80 run -- echo hello".to_owned(),
        finalization_order: finalization_order(proof_cache_manifest.is_some()),
        release_material,
        reinstall: None,
    })
}

fn adoption_command(bundle_url: &str, install_root: &Path) -> String {
    format!(
        "m80 install --bundle-url {} --install-root {} --adopt-existing-config",
        shell_single_quote(bundle_url),
        shell_single_quote(install_root.display())
    )
}

fn shell_single_quote(value: impl std::fmt::Display) -> String {
    format!("'{}'", value.to_string().replace('\'', "'\\''"))
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

#[derive(Debug)]
struct PathHandoffSummary {
    installed_m80_path: String,
    installed_m80_version: String,
}

fn install_path_handoff(final_dir: &Path, bin_dir: &Path) -> Result<PathHandoffSummary, FcError> {
    fs::create_dir_all(bin_dir).map_err(|source| FcError::PathIo {
        path: bin_dir.to_path_buf(),
        source,
    })?;
    let installed_m80 = final_dir.join("bin/m80");
    let link_path = bin_dir.join("m80");
    let temp_link = bin_dir.join(format!(".m80.install.{}", std::process::id()));
    let backup_link = bin_dir.join(format!(".m80.install.previous.{}", std::process::id()));
    if temp_link.exists() {
        fs::remove_file(&temp_link).map_err(|source| FcError::PathIo {
            path: temp_link.clone(),
            source,
        })?;
    }
    if backup_link.exists() {
        fs::remove_file(&backup_link).map_err(|source| FcError::PathIo {
            path: backup_link.clone(),
            source,
        })?;
    }
    let previous_link = previous_m80_link_state(&link_path)?;
    symlink(&installed_m80, &temp_link).map_err(|source| FcError::PathIo {
        path: temp_link.clone(),
        source,
    })?;
    if previous_link.exists {
        fs::rename(&link_path, &backup_link).map_err(|source| FcError::PathIo {
            path: link_path.clone(),
            source,
        })?;
    }
    fs::rename(&temp_link, &link_path).map_err(|source| FcError::PathIo {
        path: link_path.clone(),
        source,
    })?;

    let handoff = verify_path_handoff(&link_path, bin_dir);
    if handoff.is_err() {
        restore_previous_m80_link(&link_path, &backup_link, previous_link.exists)?;
    }
    let installed_m80_version = handoff?;
    if previous_link.exists {
        fs::remove_file(&backup_link).map_err(|source| FcError::PathIo {
            path: backup_link.clone(),
            source,
        })?;
    }
    Ok(PathHandoffSummary {
        installed_m80_path: link_path.display().to_string(),
        installed_m80_version,
    })
}

#[derive(Debug)]
struct PreviousM80Link {
    exists: bool,
}

struct PreviousActiveCandidate {
    version_dir: PathBuf,
    release_tag: String,
}

fn previous_active_candidate(
    active_pointer: &Path,
) -> Result<Option<PreviousActiveCandidate>, FcError> {
    let target = match fs::read_link(active_pointer) {
        Ok(target) => target,
        Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(FcError::PathIo {
                path: active_pointer.to_path_buf(),
                source,
            });
        }
    };
    let release_tag = target
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            FcError::Config(ConfigError::InvalidValue {
                field: "install.active_pointer",
                reason: format!(
                    "active pointer target has no UTF-8 release tag: {}",
                    target.display()
                ),
            })
        })?
        .to_owned();
    Ok(Some(PreviousActiveCandidate {
        version_dir: target,
        release_tag,
    }))
}

fn previous_m80_link_state(link_path: &Path) -> Result<PreviousM80Link, FcError> {
    match link_path.symlink_metadata() {
        Ok(metadata) => {
            let kind = metadata.file_type();
            if kind.is_file() || kind.is_symlink() {
                Ok(PreviousM80Link { exists: true })
            } else {
                Err(FcError::Config(ConfigError::InvalidValue {
                    field: "install.bin_dir",
                    reason: format!(
                        "{} already exists but is not a file or symlink",
                        link_path.display()
                    ),
                }))
            }
        }
        Err(source) if source.kind() == io::ErrorKind::NotFound => {
            Ok(PreviousM80Link { exists: false })
        }
        Err(source) => Err(FcError::PathIo {
            path: link_path.to_path_buf(),
            source,
        }),
    }
}

fn verify_path_handoff(link_path: &Path, bin_dir: &Path) -> Result<String, FcError> {
    match command_v_m80()? {
        Some(resolved) if resolved == link_path => installed_m80_version(link_path),
        Some(resolved) => Err(path_handoff_error(bin_dir, link_path, Some(&resolved))),
        None => Err(path_handoff_error(bin_dir, link_path, None)),
    }
}

fn path_handoff_error(bin_dir: &Path, link_path: &Path, resolved: Option<&Path>) -> FcError {
    let observed = resolved
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "no m80 on PATH".to_owned());
    FcError::Config(ConfigError::InvalidValue {
        field: "install.bin_dir",
        reason: format!(
            "PATH handoff failed: command -v m80 resolved {observed} but expected {}; repair with: export PATH={}:$PATH",
            link_path.display(),
            bin_dir.display()
        ),
    })
}

fn command_v_m80() -> Result<Option<PathBuf>, FcError> {
    let output = Command::new("sh")
        .arg("-c")
        .arg("command -v m80")
        .output()
        .map_err(|source| FcError::CommandSpawnFailed {
            command: "resolve installed m80",
            source,
        })?;
    if !output.status.success() {
        return Ok(None);
    }
    Ok(Some(PathBuf::from(
        String::from_utf8_lossy(&output.stdout).trim().to_owned(),
    )))
}

fn installed_m80_version(path: &Path) -> Result<String, FcError> {
    let output = Command::new(path)
        .arg("--version")
        .output()
        .map_err(|source| FcError::CommandSpawnFailed {
            command: "installed m80 --version",
            source,
        })?;
    if !output.status.success() {
        return Err(FcError::CommandFailed {
            command: "installed m80 --version",
            status: output.status,
            output: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn restore_previous_m80_link(
    link_path: &Path,
    backup_link: &Path,
    had_previous_link: bool,
) -> Result<(), FcError> {
    match fs::remove_file(link_path) {
        Ok(()) => {}
        Err(source) if source.kind() == io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(FcError::PathIo {
                path: link_path.to_path_buf(),
                source,
            });
        }
    }
    if had_previous_link {
        fs::rename(backup_link, link_path).map_err(|source| FcError::PathIo {
            path: link_path.to_path_buf(),
            source,
        })?;
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct InstallSelectorPaths {
    profile_dir: PathBuf,
    config_path: PathBuf,
}

impl InstallSelectorPaths {
    fn default_profile_path(&self) -> PathBuf {
        self.profile_dir.join("default.toml")
    }
}

pub(super) fn planned_version_dir(install_root: &Path, release_tag: &str) -> PathBuf {
    install_root.join("versions").join(release_tag)
}

pub(super) fn planned_host_binaries_manifest_path(
    install_root: &Path,
    release_tag: &str,
) -> PathBuf {
    planned_version_dir(install_root, release_tag)
        .join("artifacts")
        .join("host-binaries.manifest.json")
}

pub(super) fn planned_default_profile_path(install_root: &Path) -> PathBuf {
    install_selector_paths(install_root).default_profile_path()
}

pub(super) fn normalize_install_root(path: &Path) -> Result<PathBuf, FcError> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|source| FcError::PathIo {
                path: PathBuf::from("."),
                source,
            })?
            .join(path)
    };
    reject_parent_components("install_root", &absolute)?;
    let normalized = lexical_normalize(&absolute);
    reject_symlinked_install_root(&normalized)?;
    Ok(normalized)
}

fn install_selector_paths(install_root: &Path) -> InstallSelectorPaths {
    if install_root == Path::new(DEFAULT_INSTALL_ROOT) {
        return InstallSelectorPaths {
            profile_dir: PathBuf::from(DEFAULT_PROFILE_DIR),
            config_path: PathBuf::from(DEFAULT_CONFIG_PATH),
        };
    }

    InstallSelectorPaths {
        profile_dir: install_root.join("profiles"),
        config_path: install_root.join("config.toml"),
    }
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
    let selector_paths = install_selector_paths(install_root);
    let default_profile = selector_paths.default_profile_path();
    LayoutInstallSummary {
        state: "already_installed",
        release_tag: release_tag.to_owned(),
        install_root: install_root.display().to_string(),
        active_version_dir: final_dir.display().to_string(),
        version_dir: final_dir.display().to_string(),
        bundle_url: verification.bundle_url.clone(),
        installed_m80_path: Path::new(&plan.bin_dir).join("m80").display().to_string(),
        installed_m80_version: format!("m80 {release_tag}"),
        active_bundle_path: final_dir.display().to_string(),
        files_copied: 0,
        install_provenance: final_dir
            .join("artifacts")
            .join(INSTALL_PROVENANCE_FILE)
            .display()
            .to_string(),
        host_binaries_manifest: host_binaries_manifest.display().to_string(),
        default_profile: default_profile.display().to_string(),
        profile_path: default_profile.display().to_string(),
        active_pointer: plan.active_pointer.clone(),
        active_pointer_flipped: false,
        previous_active_version_dir: None,
        previous_active_release_tag: None,
        profile_written: false,
        smoke_gate: "not_run_idempotent_reinstall",
        host_prerequisite_status: "not_run_idempotent_reinstall".to_owned(),
        preflight_gate: "not_run_idempotent_reinstall",
        run_smoke_command: None,
        next_command: "m80 run -- echo hello".to_owned(),
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
        install_root: "/opt/m80".to_owned(),
        active_version_dir: "/opt/m80/versions/v0.0.0".to_owned(),
        version_dir: "/opt/m80/versions/v0.0.0".to_owned(),
        bundle_url:
            "https://github.com/moradology/m80/releases/download/v0.0.0/m80-linux-x86_64.tar.gz"
                .to_owned(),
        installed_m80_path: "/usr/local/bin/m80".to_owned(),
        installed_m80_version: "m80 v0.0.0".to_owned(),
        active_bundle_path: "/opt/m80/versions/v0.0.0".to_owned(),
        files_copied: 0,
        install_provenance: "/opt/m80/versions/v0.0.0/artifacts/install-provenance.json".to_owned(),
        host_binaries_manifest: "/opt/m80/versions/v0.0.0/artifacts/host-binaries.manifest.json"
            .to_owned(),
        default_profile: "/etc/m80/profiles/default.toml".to_owned(),
        profile_path: "/etc/m80/profiles/default.toml".to_owned(),
        active_pointer: "/opt/m80/active".to_owned(),
        active_pointer_flipped: false,
        previous_active_version_dir: None,
        previous_active_release_tag: None,
        profile_written: false,
        smoke_gate: "not_run_idempotent_reinstall",
        host_prerequisite_status: "not_run_idempotent_reinstall".to_owned(),
        preflight_gate: "not_run_idempotent_reinstall",
        run_smoke_command: None,
        next_command: "m80 run -- echo hello".to_owned(),
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

fn reject_parent_components(field: &'static str, path: &Path) -> Result<(), FcError> {
    if path
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(FcError::Config(ConfigError::InvalidValue {
            field,
            reason: format!("{field} must not contain '..': {}", path.display()),
        }));
    }
    Ok(())
}

fn lexical_normalize(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::RootDir | Component::Prefix(_) | Component::Normal(_) => {
                normalized.push(component.as_os_str());
            }
            Component::ParentDir => unreachable!("parent components rejected before normalize"),
        }
    }
    normalized
}

fn reject_symlinked_install_root(install_root: &Path) -> Result<(), FcError> {
    let mut prefix = PathBuf::new();
    for component in install_root.components() {
        prefix.push(component.as_os_str());
        match fs::symlink_metadata(&prefix) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(FcError::Config(ConfigError::InvalidValue {
                    field: "install_root",
                    reason: format!(
                        "install root must not pass through a symlink: {}",
                        prefix.display()
                    ),
                }));
            }
            Ok(metadata) if prefix == install_root && !metadata.is_dir() => {
                return Err(FcError::Config(ConfigError::InvalidValue {
                    field: "install_root",
                    reason: format!(
                        "install root must be a directory path, got {}",
                        install_root.display()
                    ),
                }));
            }
            Ok(_) => {}
            Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(source) => {
                return Err(FcError::PathIo {
                    path: prefix,
                    source,
                });
            }
        }
    }
    Ok(())
}

fn validate_existing_active_pointer(
    install_root: &Path,
    active_pointer: &Path,
) -> Result<(), FcError> {
    match fs::read_link(active_pointer) {
        Ok(target) => {
            if !target.is_absolute() {
                return Err(FcError::Config(ConfigError::InvalidValue {
                    field: "install.active_pointer",
                    reason: format!(
                        "active pointer target must be absolute, got {}",
                        target.display()
                    ),
                }));
            }
            reject_parent_components("install.active_pointer", &target)?;
            let versions_dir = install_root.join("versions");
            if !target.starts_with(&versions_dir) || target.parent() != Some(versions_dir.as_path())
            {
                return Err(FcError::Config(ConfigError::InvalidValue {
                    field: "install.active_pointer",
                    reason: format!(
                        "active pointer target must be one version directory under {}, got {}",
                        versions_dir.display(),
                        target.display()
                    ),
                }));
            }
            match fs::symlink_metadata(&target) {
                Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                    Err(FcError::Config(ConfigError::InvalidValue {
                        field: "install.active_pointer",
                        reason: format!(
                            "active pointer target must be an existing real version directory: {}",
                            target.display()
                        ),
                    }))
                }
                Ok(_) => Ok(()),
                Err(source) if source.kind() == io::ErrorKind::NotFound => {
                    Err(FcError::Config(ConfigError::InvalidValue {
                        field: "install.active_pointer",
                        reason: format!(
                            "active pointer target is stale or missing: {}",
                            target.display()
                        ),
                    }))
                }
                Err(source) => Err(FcError::PathIo {
                    path: target,
                    source,
                }),
            }
        }
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(FcError::PathIo {
            path: active_pointer.to_path_buf(),
            source,
        }),
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

struct SmokeGateResult {
    selected_gate: &'static str,
    preflight_gate: &'static str,
    run_smoke_command: Option<Vec<String>>,
}

fn verify_smoke_gate(
    plan: &InstallPlan,
    bundle_url: &str,
    final_dir: &Path,
    selector_paths: &InstallSelectorPaths,
) -> Result<SmokeGateResult, FcError> {
    match plan.smoke_gate {
        InstallSmokeGateArg::PreflightOnly => {
            let preflight_gate = verify_preflight_gate(bundle_url)?;
            Ok(SmokeGateResult {
                selected_gate: "preflight-only",
                preflight_gate,
                run_smoke_command: None,
            })
        }
        InstallSmokeGateArg::RunSmoke => {
            let preflight_gate = verify_live_preflight_for_run_smoke(bundle_url, plan, final_dir)?;
            let command = run_process_smoke(final_dir, selector_paths)?;
            Ok(SmokeGateResult {
                selected_gate: "run-smoke",
                preflight_gate,
                run_smoke_command: Some(command),
            })
        }
    }
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

fn verify_live_preflight_for_run_smoke(
    bundle_url: &str,
    plan: &InstallPlan,
    final_dir: &Path,
) -> Result<&'static str, FcError> {
    let resolved_tag = resolved_tag_from_version_dir(final_dir);

    if use_hostless_fixture_preflight(bundle_url)? {
        return Err(FcError::Config(ConfigError::InvalidValue {
            field: "install.smoke_gate",
            reason: format!(
                "selected_gate=run-smoke resolved_tag={resolved_tag} preflight_output=hostless_fixture_refused requires live KVM and cannot use hostless fixture; install_root={}; repair_command=m80 install --bundle-url {} --install-root {} --smoke-gate preflight-only",
                plan.install_root,
                shell_single_quote(bundle_url),
                shell_single_quote(&plan.install_root),
            ),
        }));
    }
    let preflight_gate = verify_preflight_gate(bundle_url)?;
    if preflight_gate != "live_preflight" {
        return Err(FcError::Config(ConfigError::InvalidValue {
            field: "install.smoke_gate",
            reason: format!(
                "selected_gate=run-smoke resolved_tag={resolved_tag} preflight_output={preflight_gate} requires live_preflight, got {preflight_gate}; install_root={}",
                plan.install_root
            ),
        }));
    }
    Ok(preflight_gate)
}

fn run_process_smoke(
    final_dir: &Path,
    selector_paths: &InstallSelectorPaths,
) -> Result<Vec<String>, FcError> {
    let installed_m80 = final_dir.join("bin/m80");
    let command = process_smoke_command(final_dir);
    let resolved_tag = resolved_tag_from_version_dir(final_dir);

    let output = Command::new(&installed_m80)
        .args(["run", "--", "echo", "hello"])
        .output()
        .map_err(|source| FcError::PathIo {
            path: installed_m80.clone(),
            source,
        })?;
    if !output.status.success() || output.stdout != b"hello\n" {
        return Err(FcError::Config(ConfigError::InvalidValue {
            field: "install.smoke_gate",
            reason: format!(
                "selected_gate=run-smoke resolved_tag={resolved_tag} preflight_output=live_preflight command={} exit_status={} stdout={} stderr={} active_profile=default config_path={} profile_dir={} repair_command=m80 preflight",
                command.join(" "),
                output
                    .status
                    .code()
                    .map_or_else(|| "signal".to_owned(), |code| code.to_string()),
                String::from_utf8_lossy(&output.stdout).trim_end(),
                String::from_utf8_lossy(&output.stderr).trim_end(),
                selector_paths.config_path.display(),
                selector_paths.profile_dir.display(),
            ),
        }));
    }
    Ok(command)
}

fn process_smoke_command(final_dir: &Path) -> Vec<String> {
    vec![
        final_dir.join("bin/m80").display().to_string(),
        "run".to_owned(),
        "--".to_owned(),
        "echo".to_owned(),
        "hello".to_owned(),
    ]
}

fn resolved_tag_from_version_dir(final_dir: &Path) -> &str {
    final_dir
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("<unknown>")
}

fn use_hostless_fixture_preflight(bundle_url: &str) -> Result<bool, FcError> {
    #[cfg(debug_assertions)]
    {
        if std::env::var_os("M80_INSTALL_TEST_HOSTLESS_OFFICIAL").is_some() {
            return Ok(true);
        }
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
    require_absolute_path("active_pointer", active_pointer)?;
    require_absolute_path("active_pointer_target", final_dir)?;
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

fn host_prerequisite_status(preflight_gate: &str) -> String {
    match preflight_gate {
        "live_preflight" => "passed:live_preflight",
        "hostless_fixture" => "passed:hostless_fixture",
        other => other,
    }
    .to_owned()
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
mod tests;
