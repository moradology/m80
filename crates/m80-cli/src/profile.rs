//! Runtime image/profile resolution for `m80 run`.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use m80_firecracker::{ConfigError, ConfigSource, EffectiveConfig, FcError};

const DEFAULT_PROFILE_FIELD: &str = "default_profile";
const BUILTIN_ENV_PROFILE: &str = "env";

mod report;

pub(crate) use report::{runtime_profile_report, RuntimeProfilePathIssue, RuntimeProfileReport};

/// Directory set used for profile-file lookup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProfileFilePaths {
    pub(crate) system_dir: Option<PathBuf>,
    pub(crate) user_dir: Option<PathBuf>,
}

impl ProfileFilePaths {
    pub(crate) fn host() -> Self {
        Self {
            system_dir: Some(PathBuf::from("/etc/m80/profiles")),
            user_dir: std::env::var_os("HOME")
                .map(PathBuf::from)
                .map(|home| home.join(".config/m80/profiles")),
        }
    }

    #[cfg(test)]
    fn search_paths(&self, name: &str) -> Vec<PathBuf> {
        let filename = profile_filename(name);
        [self.system_dir.as_ref(), self.user_dir.as_ref()]
            .into_iter()
            .flatten()
            .map(|dir| dir.join(&filename))
            .collect()
    }
}

/// Source of the resolved runtime profile body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProfileBodySource {
    BuiltinEnv,
    SystemFile,
    UserFile,
}

/// Resolved profile data needed before preflight discovers boot artifacts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RuntimeProfile {
    pub(crate) name: String,
    pub(crate) selection_source: ConfigSource,
    pub(crate) body_source: ProfileBodySource,
    pub(crate) file_path: Option<PathBuf>,
    pub(crate) artifact_dir: Option<PathBuf>,
    pub(crate) kernel_image: Option<PathBuf>,
    pub(crate) rootfs_image: Option<PathBuf>,
    pub(crate) kernel_kind: Option<String>,
    pub(crate) guestd: Option<PathBuf>,
    pub(crate) guest_manifest: Option<PathBuf>,
    pub(crate) build_receipt: Option<PathBuf>,
    pub(crate) install_provenance: Option<PathBuf>,
    pub(crate) host_binaries_manifest: Option<PathBuf>,
    pub(crate) firecracker_bin: Option<PathBuf>,
    pub(crate) firecracker_seccomp_filter: Option<PathBuf>,
    pub(crate) jailer_bin: Option<PathBuf>,
    pub(crate) jailer_harden_bin: Option<PathBuf>,
    pub(crate) net_helper_bin: Option<PathBuf>,
    pub(crate) run_root: Option<PathBuf>,
    pub(crate) release_tag: Option<String>,
    pub(crate) m80_version: Option<String>,
    pub(crate) description: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RuntimeProfileFile {
    artifact_dir: Option<PathBuf>,
    kernel_image: PathBuf,
    rootfs_image: PathBuf,
    kernel_kind: Option<String>,
    guestd: Option<PathBuf>,
    guest_manifest: Option<PathBuf>,
    build_receipt: Option<PathBuf>,
    install_provenance: Option<PathBuf>,
    host_binaries_manifest: Option<PathBuf>,
    firecracker_bin: Option<PathBuf>,
    firecracker_seccomp_filter: Option<PathBuf>,
    jailer_bin: Option<PathBuf>,
    jailer_harden_bin: Option<PathBuf>,
    net_helper_bin: Option<PathBuf>,
    run_root: Option<PathBuf>,
    release_tag: Option<String>,
    m80_version: Option<String>,
    description: Option<String>,
}

/// Resolve the profile selected by the effective `default_profile` field.
pub(crate) fn resolve_from_effective(
    effective: &EffectiveConfig,
    paths: ProfileFilePaths,
) -> Result<RuntimeProfile, FcError> {
    let selected = effective
        .fields
        .iter()
        .find(|field| field.name == DEFAULT_PROFILE_FIELD)
        .ok_or(FcError::Config(ConfigError::MissingField {
            field: DEFAULT_PROFILE_FIELD,
        }))?;
    resolve_named_profile(&selected.value, selected.source, paths)
}

fn resolve_named_profile(
    name: &str,
    selection_source: ConfigSource,
    paths: ProfileFilePaths,
) -> Result<RuntimeProfile, FcError> {
    validate_profile_name(name)?;

    if name == BUILTIN_ENV_PROFILE {
        return Ok(RuntimeProfile {
            name: name.to_owned(),
            selection_source,
            body_source: ProfileBodySource::BuiltinEnv,
            file_path: None,
            artifact_dir: None,
            kernel_image: None,
            rootfs_image: None,
            kernel_kind: None,
            guestd: None,
            guest_manifest: None,
            build_receipt: None,
            install_provenance: None,
            host_binaries_manifest: None,
            firecracker_bin: None,
            firecracker_seccomp_filter: None,
            jailer_bin: None,
            jailer_harden_bin: None,
            net_helper_bin: None,
            run_root: None,
            release_tag: None,
            m80_version: None,
            description: Some("boot artifacts resolved from M80_* environment/defaults".to_owned()),
        });
    }

    let filename = profile_filename(name);
    let mut selected_path = None;
    if let Some(system_dir) = paths.system_dir {
        let path = system_dir.join(&filename);
        if path.exists() {
            selected_path = Some((ProfileBodySource::SystemFile, path));
        }
    }
    if let Some(user_dir) = paths.user_dir {
        let path = user_dir.join(&filename);
        if path.exists() {
            selected_path = Some((ProfileBodySource::UserFile, path));
        }
    }

    let Some((body_source, file_path)) = selected_path else {
        return Err(FcError::Config(ConfigError::InvalidValue {
            field: "default_profile",
            reason: format!(
                "runtime profile {name:?} not found; searched /etc/m80/profiles/{name}.toml and ~/.config/m80/profiles/{name}.toml"
            ),
        }));
    };

    let raw = std::fs::read_to_string(&file_path).map_err(|e| FcError::PathIo {
        path: file_path.clone(),
        source: e,
    })?;
    let parsed: RuntimeProfileFile = toml::from_str(&raw).map_err(|source| {
        FcError::Config(ConfigError::TomlSyntax {
            layer: "runtime profile",
            path: file_path.clone(),
            source,
        })
    })?;
    validate_absolute_path("kernel_image", &parsed.kernel_image)?;
    validate_absolute_path("rootfs_image", &parsed.rootfs_image)?;
    for (field, path) in [
        ("artifact_dir", parsed.artifact_dir.as_deref()),
        ("guestd", parsed.guestd.as_deref()),
        ("guest_manifest", parsed.guest_manifest.as_deref()),
        ("build_receipt", parsed.build_receipt.as_deref()),
        ("install_provenance", parsed.install_provenance.as_deref()),
        (
            "host_binaries_manifest",
            parsed.host_binaries_manifest.as_deref(),
        ),
        ("firecracker_bin", parsed.firecracker_bin.as_deref()),
        (
            "firecracker_seccomp_filter",
            parsed.firecracker_seccomp_filter.as_deref(),
        ),
        ("jailer_bin", parsed.jailer_bin.as_deref()),
        ("jailer_harden_bin", parsed.jailer_harden_bin.as_deref()),
        ("net_helper_bin", parsed.net_helper_bin.as_deref()),
        ("run_root", parsed.run_root.as_deref()),
    ] {
        if let Some(path) = path {
            validate_absolute_path(field, path)?;
        }
    }
    if let Some(kind) = parsed.kernel_kind.as_deref() {
        validate_kernel_kind(kind)?;
    }

    Ok(RuntimeProfile {
        name: name.to_owned(),
        selection_source,
        body_source,
        file_path: Some(file_path),
        artifact_dir: parsed.artifact_dir,
        kernel_image: Some(parsed.kernel_image),
        rootfs_image: Some(parsed.rootfs_image),
        kernel_kind: parsed.kernel_kind,
        guestd: parsed.guestd,
        guest_manifest: parsed.guest_manifest,
        build_receipt: parsed.build_receipt,
        install_provenance: parsed.install_provenance,
        host_binaries_manifest: parsed.host_binaries_manifest,
        firecracker_bin: parsed.firecracker_bin,
        firecracker_seccomp_filter: parsed.firecracker_seccomp_filter,
        jailer_bin: parsed.jailer_bin,
        jailer_harden_bin: parsed.jailer_harden_bin,
        net_helper_bin: parsed.net_helper_bin,
        run_root: parsed.run_root,
        release_tag: parsed.release_tag,
        m80_version: parsed.m80_version,
        description: parsed.description,
    })
}

fn validate_profile_name(name: &str) -> Result<(), FcError> {
    if name.is_empty() {
        return Err(FcError::Config(ConfigError::InvalidValue {
            field: "default_profile",
            reason: "runtime profile name must not be empty".to_owned(),
        }));
    }
    if name == "." || name == ".." || name.contains('/') || name.contains('\\') {
        return Err(FcError::Config(ConfigError::InvalidValue {
            field: "default_profile",
            reason: format!("runtime profile name {name:?} must be a single path segment"),
        }));
    }
    if Path::new(name).is_absolute() {
        return Err(FcError::Config(ConfigError::InvalidValue {
            field: "default_profile",
            reason: format!("runtime profile name {name:?} must not be absolute"),
        }));
    }
    Ok(())
}

fn validate_kernel_kind(kind: &str) -> Result<(), FcError> {
    match kind {
        "stock" | "stripped" => Ok(()),
        other => Err(FcError::Config(ConfigError::InvalidValue {
            field: "kernel_kind",
            reason: format!("runtime profile kernel_kind must be stock|stripped, got {other:?}"),
        })),
    }
}

fn validate_absolute_path(field: &'static str, path: &Path) -> Result<(), FcError> {
    if path.is_absolute() {
        return Ok(());
    }
    Err(FcError::Config(ConfigError::InvalidValue {
        field,
        reason: format!(
            "{field} must be an absolute host path, got {}",
            path.display()
        ),
    }))
}

fn profile_filename(name: &str) -> String {
    format!("{name}.toml")
}

#[cfg(test)]
mod tests;
