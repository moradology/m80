//! Read-only installed-state resolver for status and freshness surfaces.

#![cfg_attr(not(test), allow(dead_code))]

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

use m80_firecracker::{load_config_from_paths, ConfigFilePaths, ConfigSource, EffectiveConfig};
use serde::Serialize;

use crate::profile::{self, ProfileBodySource, ProfileFilePaths, RuntimeProfile};

mod active;
mod metadata;
mod state;

pub(crate) use active::{ActivePointerReport, ActivePointerStatus};

const DEFAULT_PROFILE_FIELD: &str = "default_profile";
const ACTIVE_POINTER_NAME: &str = "active";
const VERSIONS_DIR_NAME: &str = "versions";
const ARTIFACTS_DIR_NAME: &str = "artifacts";

#[derive(Debug, Clone)]
pub(crate) struct InstallStateRequest {
    pub(crate) paths: InstallStatePaths,
    pub(crate) profile_override: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct InstallStatePaths {
    pub(crate) install_root: PathBuf,
    pub(crate) config_paths: ConfigFilePaths,
    pub(crate) profile_paths: ProfileFilePaths,
}

impl InstallStatePaths {
    pub(crate) fn host(install_root: impl Into<PathBuf>) -> Self {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        Self {
            install_root: install_root.into(),
            config_paths: ConfigFilePaths {
                system: Some(PathBuf::from("/etc/m80/config.toml")),
                system_drop_in_dir: Some(PathBuf::from("/etc/m80/config.d")),
                user: home
                    .as_ref()
                    .map(|home| home.join(".config/m80/config.toml")),
                user_drop_in_dir: home.map(|home| home.join(".config/m80/config.d")),
            },
            profile_paths: ProfileFilePaths::host(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct InstallStateReport {
    pub(crate) state: InstallStateKind,
    pub(crate) install_root: PathBuf,
    pub(crate) active_pointer: ActivePointerReport,
    pub(crate) config: InstallConfigReport,
    pub(crate) profile: Option<InstallProfileReport>,
    pub(crate) metadata: Option<metadata::InstallMetadataReport>,
    pub(crate) diagnostics: Vec<InstallStateDiagnostic>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum InstallStateKind {
    HealthyActiveRelease,
    MissingActivePointer,
    DanglingActivePointer,
    LocalDevTree,
    StaleProfileTarget,
    ExplicitOverride,
    MissingInstallMetadata,
    StaleInstallMetadata,
    TamperedProofCache,
    InvalidInstallMetadata,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct InstallConfigReport {
    pub(crate) system_path: Option<PathBuf>,
    pub(crate) system_drop_in_dir: Option<PathBuf>,
    pub(crate) user_path: Option<PathBuf>,
    pub(crate) user_drop_in_dir: Option<PathBuf>,
    pub(crate) default_profile: Option<String>,
    pub(crate) default_profile_source: Option<ConfigSource>,
    pub(crate) explicit_override: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct InstallProfileReport {
    pub(crate) name: String,
    pub(crate) selection_source: ConfigSource,
    pub(crate) body_source: &'static str,
    pub(crate) file_path: Option<PathBuf>,
    pub(crate) artifact_dir: Option<PathBuf>,
    pub(crate) version_dir: Option<PathBuf>,
    pub(crate) release_tag: Option<String>,
    pub(crate) m80_version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct InstallStateDiagnostic {
    pub(crate) code: InstallStateDiagnosticCode,
    pub(crate) field: Option<&'static str>,
    pub(crate) path: Option<PathBuf>,
    pub(crate) message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum InstallStateDiagnosticCode {
    ConfigLoadFailed,
    DefaultProfileMissing,
    ProfileLoadFailed,
    MissingActivePointer,
    DanglingActivePointer,
    ActivePointerUnreadable,
    ActivePointerTraversal,
    ActivePointerOutsideInstallRoot,
    ActivePointerNotVersionDir,
    ProfilePathTraversal,
    ProfilePathOutsideInstallRoot,
    ProfileArtifactDirMalformed,
    ProfileTargetsInactiveVersion,
    InstallMetadataMissing,
    InstallMetadataInvalid,
    InstallMetadataStale,
    ProofCacheMissing,
    ProofCacheInvalid,
    ProofCacheStale,
    ExplicitProfileOverride,
    LocalDevProfile,
}

pub(crate) fn resolve_install_state(request: InstallStateRequest) -> InstallStateReport {
    let install_root = request.paths.install_root;
    let active_pointer_path = install_root.join(ACTIVE_POINTER_NAME);
    let mut diagnostics = Vec::new();
    let active_pointer =
        active::read_active_pointer(&install_root, &active_pointer_path, &mut diagnostics);

    let mut overrides = HashMap::new();
    if let Some(profile_override) = request.profile_override {
        overrides.insert(DEFAULT_PROFILE_FIELD.to_owned(), profile_override);
    }
    let config_result = load_config_from_paths(overrides, request.paths.config_paths.clone());
    let (config, effective) = match config_result {
        Ok(effective) => (
            config_report(&request.paths.config_paths, Some(&effective)),
            Some(effective),
        ),
        Err(err) => {
            diagnostics.push(diagnostic(
                InstallStateDiagnosticCode::ConfigLoadFailed,
                None,
                None,
                format!("{err:#}"),
            ));
            (config_report(&request.paths.config_paths, None), None)
        }
    };

    let profile = effective.as_ref().and_then(|effective| {
        let default_profile = effective_default_profile(effective);
        if default_profile.is_none() {
            diagnostics.push(diagnostic(
                InstallStateDiagnosticCode::DefaultProfileMissing,
                Some(DEFAULT_PROFILE_FIELD),
                None,
                "effective config did not contain default_profile".to_owned(),
            ));
        }
        match profile::resolve_from_effective(effective, request.paths.profile_paths) {
            Ok(profile) => Some(profile),
            Err(err) => {
                diagnostics.push(diagnostic(
                    InstallStateDiagnosticCode::ProfileLoadFailed,
                    Some(DEFAULT_PROFILE_FIELD),
                    None,
                    format!("{err:#}"),
                ));
                None
            }
        }
    });

    let profile_report = profile.as_ref().map(|profile| {
        profile_report(
            &install_root,
            profile,
            config.explicit_override,
            &mut diagnostics,
        )
    });
    if let (ActivePointerStatus::Live, Some(active_version), Some(profile)) = (
        active_pointer.status,
        active_pointer.version_dir.as_deref(),
        profile_report.as_ref(),
    ) {
        if profile.version_dir.as_deref() != Some(active_version) {
            diagnostics.push(diagnostic(
                InstallStateDiagnosticCode::ProfileTargetsInactiveVersion,
                Some("artifact_dir"),
                profile.version_dir.clone(),
                format!(
                    "selected profile points at a different version than active pointer: active={} profile={}",
                    active_version.display(),
                    profile
                        .version_dir
                        .as_deref()
                        .map(|path| path.display().to_string())
                        .unwrap_or_else(|| "unavailable".to_owned())
                ),
            ));
        }
    }
    let metadata = if state::should_read_metadata(&active_pointer, profile_report.as_ref()) {
        profile.as_ref().and_then(|profile| {
            profile_report.as_ref().map(|profile_report| {
                metadata::read_install_metadata(profile, profile_report, &mut diagnostics)
            })
        })
    } else {
        None
    };
    let state = state::classify_install_state(
        &active_pointer,
        &config,
        profile_report.as_ref(),
        &diagnostics,
    );

    InstallStateReport {
        state,
        install_root,
        active_pointer,
        config,
        profile: profile_report,
        metadata,
        diagnostics,
    }
}

fn config_report(
    paths: &ConfigFilePaths,
    effective: Option<&EffectiveConfig>,
) -> InstallConfigReport {
    let default = effective.and_then(effective_default_profile);
    let explicit_override = default
        .as_ref()
        .is_some_and(|(_, source)| is_explicit_override_source(*source));
    InstallConfigReport {
        system_path: paths.system.clone(),
        system_drop_in_dir: paths.system_drop_in_dir.clone(),
        user_path: paths.user.clone(),
        user_drop_in_dir: paths.user_drop_in_dir.clone(),
        default_profile: default.as_ref().map(|(value, _)| value.clone()),
        default_profile_source: default.map(|(_, source)| source),
        explicit_override,
    }
}

fn effective_default_profile(effective: &EffectiveConfig) -> Option<(String, ConfigSource)> {
    effective
        .fields
        .iter()
        .find(|field| field.name == DEFAULT_PROFILE_FIELD)
        .map(|field| (field.value.clone(), field.source))
}

fn profile_report(
    install_root: &Path,
    profile: &RuntimeProfile,
    explicit_override: bool,
    diagnostics: &mut Vec<InstallStateDiagnostic>,
) -> InstallProfileReport {
    if explicit_override {
        diagnostics.push(diagnostic(
            InstallStateDiagnosticCode::ExplicitProfileOverride,
            Some(DEFAULT_PROFILE_FIELD),
            profile.file_path.clone(),
            format!(
                "default_profile was selected from {:?}; installed active state is not authoritative for this invocation",
                profile.selection_source
            ),
        ));
    }
    if profile.body_source == ProfileBodySource::BuiltinEnv {
        diagnostics.push(diagnostic(
            InstallStateDiagnosticCode::LocalDevProfile,
            Some(DEFAULT_PROFILE_FIELD),
            None,
            "selected profile resolves artifacts from environment/defaults".to_owned(),
        ));
    }

    if !explicit_override && profile.body_source != ProfileBodySource::BuiltinEnv {
        validate_installed_profile_paths(install_root, profile, diagnostics);
    }
    let version_dir = profile
        .artifact_dir
        .as_deref()
        .and_then(version_dir_from_artifact_dir);
    InstallProfileReport {
        name: profile.name.clone(),
        selection_source: profile.selection_source,
        body_source: profile_body_source_label(profile.body_source),
        file_path: profile.file_path.clone(),
        artifact_dir: profile.artifact_dir.clone(),
        version_dir,
        release_tag: profile.release_tag.clone(),
        m80_version: profile.m80_version.clone(),
    }
}

fn validate_installed_profile_paths(
    install_root: &Path,
    profile: &RuntimeProfile,
    diagnostics: &mut Vec<InstallStateDiagnostic>,
) {
    for (field, path) in [
        ("artifact_dir", profile.artifact_dir.as_deref()),
        ("kernel_image", profile.kernel_image.as_deref()),
        ("rootfs_image", profile.rootfs_image.as_deref()),
        ("guestd", profile.guestd.as_deref()),
        ("guest_manifest", profile.guest_manifest.as_deref()),
        ("build_receipt", profile.build_receipt.as_deref()),
        ("install_provenance", profile.install_provenance.as_deref()),
        (
            "host_binaries_manifest",
            profile.host_binaries_manifest.as_deref(),
        ),
        ("jailer_harden_bin", profile.jailer_harden_bin.as_deref()),
        ("net_helper_bin", profile.net_helper_bin.as_deref()),
    ] {
        if let Some(path) = path {
            validate_profile_install_path(install_root, field, path, diagnostics);
        }
    }
    for (field, path) in [
        ("firecracker_bin", profile.firecracker_bin.as_deref()),
        (
            "firecracker_seccomp_filter",
            profile.firecracker_seccomp_filter.as_deref(),
        ),
        ("jailer_bin", profile.jailer_bin.as_deref()),
        ("run_root", profile.run_root.as_deref()),
    ] {
        if let Some(path) = path {
            validate_profile_path_has_no_traversal(field, path, diagnostics);
        }
    }
    match profile
        .artifact_dir
        .as_deref()
        .and_then(version_dir_from_artifact_dir)
    {
        Some(_) => {}
        None => diagnostics.push(diagnostic(
            InstallStateDiagnosticCode::ProfileArtifactDirMalformed,
            Some("artifact_dir"),
            profile.artifact_dir.clone(),
            "profile artifact_dir must be <install-root>/versions/<tag>/artifacts".to_owned(),
        )),
    }
}

fn validate_profile_install_path(
    install_root: &Path,
    field: &'static str,
    path: &Path,
    diagnostics: &mut Vec<InstallStateDiagnostic>,
) {
    if !validate_profile_path_has_no_traversal(field, path, diagnostics) {
        return;
    }
    if !path.starts_with(install_root) {
        diagnostics.push(diagnostic(
            InstallStateDiagnosticCode::ProfilePathOutsideInstallRoot,
            Some(field),
            Some(path.to_path_buf()),
            format!(
                "profile path must stay under install root {}, got {}",
                install_root.display(),
                path.display()
            ),
        ));
    }
}

fn validate_profile_path_has_no_traversal(
    field: &'static str,
    path: &Path,
    diagnostics: &mut Vec<InstallStateDiagnostic>,
) -> bool {
    if !path_has_parent_component(path) {
        return true;
    }
    diagnostics.push(diagnostic(
        InstallStateDiagnosticCode::ProfilePathTraversal,
        Some(field),
        Some(path.to_path_buf()),
        format!("profile path contains '..': {}", path.display()),
    ));
    false
}

fn is_explicit_override_source(source: ConfigSource) -> bool {
    matches!(
        source,
        ConfigSource::Env
            | ConfigSource::Flag
            | ConfigSource::UserFile
            | ConfigSource::UserDropIn
            | ConfigSource::SystemDropIn
    )
}

fn version_dir_from_artifact_dir(artifact_dir: &Path) -> Option<PathBuf> {
    if artifact_dir.file_name().and_then(|name| name.to_str()) != Some(ARTIFACTS_DIR_NAME) {
        return None;
    }
    let version_dir = artifact_dir.parent()?;
    if version_dir
        .parent()
        .and_then(|path| path.file_name())
        .and_then(|name| name.to_str())
        != Some(VERSIONS_DIR_NAME)
    {
        return None;
    }
    Some(version_dir.to_path_buf())
}

fn release_tag_from_version_dir(version_dir: &Path) -> Option<String> {
    version_dir
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_owned)
}

fn profile_body_source_label(source: ProfileBodySource) -> &'static str {
    match source {
        ProfileBodySource::BuiltinEnv => "builtin_env",
        ProfileBodySource::SystemFile => "system_file",
        ProfileBodySource::UserFile => "user_file",
    }
}

fn path_has_parent_component(path: &Path) -> bool {
    path.components()
        .any(|component| matches!(component, Component::ParentDir))
}

fn diagnostic(
    code: InstallStateDiagnosticCode,
    field: Option<&'static str>,
    path: Option<PathBuf>,
    message: String,
) -> InstallStateDiagnostic {
    InstallStateDiagnostic {
        code,
        field,
        path,
        message,
    }
}

#[cfg(test)]
mod tests;
