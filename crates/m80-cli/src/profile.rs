//! Runtime image/profile resolution for `m80 run`.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use m80_firecracker::{ConfigError, ConfigSource, EffectiveConfig, FcError};
use m80_preflight::{ENV_KERNEL_IMAGE, ENV_KERNEL_KIND, ENV_ROOTFS_IMAGE};

const DEFAULT_PROFILE_FIELD: &str = "default_profile";
const BUILTIN_ENV_PROFILE: &str = "env";

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
    pub(crate) kernel_image: Option<PathBuf>,
    pub(crate) rootfs_image: Option<PathBuf>,
    pub(crate) kernel_kind: Option<String>,
    pub(crate) description: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RuntimeProfileFile {
    kernel_image: PathBuf,
    rootfs_image: PathBuf,
    kernel_kind: Option<String>,
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
            kernel_image: None,
            rootfs_image: None,
            kernel_kind: None,
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
    if let Some(kind) = parsed.kernel_kind.as_deref() {
        validate_kernel_kind(kind)?;
    }

    Ok(RuntimeProfile {
        name: name.to_owned(),
        selection_source,
        body_source,
        file_path: Some(file_path),
        kernel_image: Some(parsed.kernel_image),
        rootfs_image: Some(parsed.rootfs_image),
        kernel_kind: parsed.kernel_kind,
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

impl RuntimeProfile {
    pub(crate) fn apply_env(&self) -> AppliedProfileEnv {
        let overrides = self.env_overrides();
        let previous = overrides
            .iter()
            .map(|(key, _)| (*key, std::env::var_os(key)))
            .collect();
        for (key, value) in overrides {
            std::env::set_var(key, value);
        }
        AppliedProfileEnv { previous }
    }

    fn env_overrides(&self) -> Vec<(&'static str, OsString)> {
        let mut overrides = Vec::new();
        if let Some(path) = &self.kernel_image {
            overrides.push((ENV_KERNEL_IMAGE, path.as_os_str().to_owned()));
        }
        if let Some(path) = &self.rootfs_image {
            overrides.push((ENV_ROOTFS_IMAGE, path.as_os_str().to_owned()));
        }
        if let Some(kind) = &self.kernel_kind {
            overrides.push((ENV_KERNEL_KIND, OsString::from(kind)));
        }
        overrides
    }
}

/// Restores any artifact env vars overlaid for profile-aware preflight.
pub(crate) struct AppliedProfileEnv {
    previous: Vec<(&'static str, Option<OsString>)>,
}

impl Drop for AppliedProfileEnv {
    fn drop(&mut self) {
        for (key, value) in &self.previous {
            if let Some(value) = value {
                std::env::set_var(key, value);
            } else {
                std::env::remove_var(key);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Mutex, OnceLock};

    use tempfile::TempDir;

    use super::*;
    use m80_firecracker::{ConfigSource, EffectiveField};

    fn effective_profile(name: &str, source: ConfigSource) -> EffectiveConfig {
        EffectiveConfig {
            fields: vec![EffectiveField {
                name: DEFAULT_PROFILE_FIELD.to_owned(),
                value: name.to_owned(),
                source,
            }],
        }
    }

    fn paths(system: &TempDir, user: &TempDir) -> ProfileFilePaths {
        ProfileFilePaths {
            system_dir: Some(system.path().to_path_buf()),
            user_dir: Some(user.path().to_path_buf()),
        }
    }

    fn write_profile(dir: &TempDir, name: &str, content: &str) {
        std::fs::write(dir.path().join(profile_filename(name)), content).unwrap();
    }

    #[test]
    fn default_env_profile_requires_no_profile_file() {
        let system = tempfile::tempdir().unwrap();
        let user = tempfile::tempdir().unwrap();

        let profile = resolve_from_effective(
            &effective_profile("env", ConfigSource::Default),
            paths(&system, &user),
        )
        .unwrap();

        assert_eq!(profile.name, "env");
        assert_eq!(profile.selection_source, ConfigSource::Default);
        assert_eq!(profile.body_source, ProfileBodySource::BuiltinEnv);
        assert!(profile.file_path.is_none());
        assert!(profile.kernel_image.is_none());
        assert!(profile.rootfs_image.is_none());
    }

    #[test]
    fn explicit_profile_loads_user_file_over_system_file() {
        let system = tempfile::tempdir().unwrap();
        let user = tempfile::tempdir().unwrap();
        write_profile(
            &system,
            "dev",
            "kernel_image = \"/system/vmlinux\"\nrootfs_image = \"/system/rootfs.ext4\"\n",
        );
        write_profile(
            &user,
            "dev",
            "kernel_image = \"/user/vmlinux\"\nrootfs_image = \"/user/rootfs.ext4\"\nkernel_kind = \"stripped\"\ndescription = \"developer shell\"\n",
        );

        let profile = resolve_from_effective(
            &effective_profile("dev", ConfigSource::Flag),
            paths(&system, &user),
        )
        .unwrap();

        assert_eq!(profile.name, "dev");
        assert_eq!(profile.selection_source, ConfigSource::Flag);
        assert_eq!(profile.body_source, ProfileBodySource::UserFile);
        assert_eq!(
            profile.kernel_image.as_deref(),
            Some(Path::new("/user/vmlinux"))
        );
        assert_eq!(
            profile.rootfs_image.as_deref(),
            Some(Path::new("/user/rootfs.ext4"))
        );
        assert_eq!(profile.kernel_kind.as_deref(), Some("stripped"));
        assert_eq!(profile.description.as_deref(), Some("developer shell"));
    }

    #[test]
    fn missing_profile_fails_before_preflight() {
        let system = tempfile::tempdir().unwrap();
        let user = tempfile::tempdir().unwrap();

        let err = resolve_from_effective(
            &effective_profile("missing", ConfigSource::Flag),
            paths(&system, &user),
        )
        .unwrap_err();

        assert!(
            err.to_string()
                .contains("runtime profile \"missing\" not found"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn unknown_profile_keys_fail_closed() {
        let system = tempfile::tempdir().unwrap();
        let user = tempfile::tempdir().unwrap();
        write_profile(
            &user,
            "dev",
            "kernel_image = \"/vmlinux\"\nrootfs_image = \"/rootfs.ext4\"\nextra = true\n",
        );

        let err = resolve_from_effective(
            &effective_profile("dev", ConfigSource::Flag),
            paths(&system, &user),
        )
        .unwrap_err();

        assert!(
            err.to_string().contains("unknown field `extra`"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn invalid_kernel_kind_fails_closed() {
        let system = tempfile::tempdir().unwrap();
        let user = tempfile::tempdir().unwrap();
        write_profile(
            &user,
            "dev",
            "kernel_image = \"/vmlinux\"\nrootfs_image = \"/rootfs.ext4\"\nkernel_kind = \"tiny\"\n",
        );

        let err = resolve_from_effective(
            &effective_profile("dev", ConfigSource::Flag),
            paths(&system, &user),
        )
        .unwrap_err();

        assert!(
            err.to_string()
                .contains("kernel_kind must be stock|stripped"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn path_like_profile_names_fail_closed() {
        let system = tempfile::tempdir().unwrap();
        let user = tempfile::tempdir().unwrap();

        let err = resolve_from_effective(
            &effective_profile("../dev", ConfigSource::Flag),
            paths(&system, &user),
        )
        .unwrap_err();

        assert!(
            err.to_string().contains("single path segment"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn profile_paths_are_deterministic() {
        let paths = ProfileFilePaths {
            system_dir: Some(PathBuf::from("/etc/m80/profiles")),
            user_dir: Some(PathBuf::from("/home/alice/.config/m80/profiles")),
        };

        assert_eq!(
            paths.search_paths("dev"),
            vec![
                PathBuf::from("/etc/m80/profiles/dev.toml"),
                PathBuf::from("/home/alice/.config/m80/profiles/dev.toml"),
            ]
        );
    }

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn profile_env_overlay_reaches_preflight_artifact_inputs() {
        let _lock = env_lock().lock().unwrap();
        struct RestoreEnv {
            kernel: Option<OsString>,
            rootfs: Option<OsString>,
            kind: Option<OsString>,
        }
        impl Drop for RestoreEnv {
            fn drop(&mut self) {
                restore_key(ENV_KERNEL_IMAGE, &self.kernel);
                restore_key(ENV_ROOTFS_IMAGE, &self.rootfs);
                restore_key(ENV_KERNEL_KIND, &self.kind);
            }
        }
        fn restore_key(key: &'static str, value: &Option<OsString>) {
            if let Some(value) = value {
                std::env::set_var(key, value);
            } else {
                std::env::remove_var(key);
            }
        }

        let _restore = RestoreEnv {
            kernel: std::env::var_os(ENV_KERNEL_IMAGE),
            rootfs: std::env::var_os(ENV_ROOTFS_IMAGE),
            kind: std::env::var_os(ENV_KERNEL_KIND),
        };
        std::env::remove_var(ENV_KERNEL_IMAGE);
        std::env::remove_var(ENV_ROOTFS_IMAGE);
        std::env::remove_var(ENV_KERNEL_KIND);

        let profile = RuntimeProfile {
            name: "dev".to_owned(),
            selection_source: ConfigSource::Flag,
            body_source: ProfileBodySource::UserFile,
            file_path: Some(PathBuf::from("/profiles/dev.toml")),
            kernel_image: Some(PathBuf::from("/images/vmlinux")),
            rootfs_image: Some(PathBuf::from("/images/rootfs.ext4")),
            kernel_kind: Some("stripped".to_owned()),
            description: None,
        };

        {
            let _overlay = profile.apply_env();
            assert_eq!(std::env::var(ENV_KERNEL_IMAGE).unwrap(), "/images/vmlinux");
            assert_eq!(
                std::env::var(ENV_ROOTFS_IMAGE).unwrap(),
                "/images/rootfs.ext4"
            );
            assert_eq!(std::env::var(ENV_KERNEL_KIND).unwrap(), "stripped");
        }

        assert!(std::env::var_os(ENV_KERNEL_IMAGE).is_none());
        assert!(std::env::var_os(ENV_ROOTFS_IMAGE).is_none());
        assert!(std::env::var_os(ENV_KERNEL_KIND).is_none());
    }
}
