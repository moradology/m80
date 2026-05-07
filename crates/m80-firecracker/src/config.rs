//! Configuration loading: defaults → /etc/m80/config.toml → /etc/m80/config.d
//! → ~/.config/m80/config.toml → ~/.config/m80/config.d → M80_* env vars
//! → caller-supplied flag overrides. Returns an [`EffectiveConfig`] with each
//! field tagged by its winning source.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::error::FcError;
use crate::types::{BackendConfig, CgroupMode, ConfigSource, EffectiveConfig, EffectiveField};

/// Field names recognized by the loader.
mod field {
    pub const DEFAULT_PROFILE: &str = "default_profile";
    pub const MAX_CONCURRENT_VMS: &str = "max_concurrent_vms";
    pub const RUN_ROOT: &str = "run_root";
    pub const JAIL_UID: &str = "jail_uid";
    pub const JAIL_GID: &str = "jail_gid";
    pub const CGROUP_MODE: &str = "cgroup_mode";
}

/// Built-in defaults for every recognized field.
fn defaults() -> HashMap<String, (String, ConfigSource)> {
    let mut m = HashMap::new();
    m.insert(
        field::DEFAULT_PROFILE.into(),
        ("env".into(), ConfigSource::Default),
    );
    m.insert(
        field::MAX_CONCURRENT_VMS.into(),
        ("8".into(), ConfigSource::Default),
    );
    m.insert(
        field::RUN_ROOT.into(),
        ("/var/run/m80".into(), ConfigSource::Default),
    );
    m.insert(
        field::JAIL_UID.into(),
        ("3000".into(), ConfigSource::Default),
    );
    m.insert(
        field::JAIL_GID.into(),
        ("3000".into(), ConfigSource::Default),
    );
    m.insert(
        field::CGROUP_MODE.into(),
        ("unified-v2".into(), ConfigSource::Default),
    );
    m
}

/// File paths used by configuration loading.
///
/// `None` means that layer is skipped. Production callers use
/// [`ConfigFilePaths::host`]. Tests that need hermetic config behavior pass
/// explicit temporary paths instead of reading `/etc` or the real home
/// directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigFilePaths {
    /// System-level config file path.
    pub system: Option<PathBuf>,
    /// System-level drop-in config directory.
    pub system_drop_in_dir: Option<PathBuf>,
    /// User-level config file path.
    pub user: Option<PathBuf>,
    /// User-level drop-in config directory.
    pub user_drop_in_dir: Option<PathBuf>,
}

impl ConfigFilePaths {
    /// Host config paths: `/etc/m80/config.toml`, `/etc/m80/config.d`,
    /// `~/.config/m80/config.toml`, and `~/.config/m80/config.d` when `HOME`
    /// is set.
    pub fn host() -> Self {
        Self {
            system: Some(PathBuf::from("/etc/m80/config.toml")),
            system_drop_in_dir: Some(PathBuf::from("/etc/m80/config.d")),
            user: home_dir().map(|home| home.join(".config/m80/config.toml")),
            user_drop_in_dir: home_dir().map(|home| home.join(".config/m80/config.d")),
        }
    }
}

/// Load configuration from the layered sources and return `EffectiveConfig`.
///
/// `args_overrides` is a map of `field_name → value` representing CLI flags.
pub fn load(args_overrides: HashMap<String, String>) -> Result<EffectiveConfig, FcError> {
    load_from_paths(args_overrides, ConfigFilePaths::host())
}

/// Load configuration from explicit file paths.
///
/// Loading order is the same as [`load`]: built-in defaults, optional system
/// file, optional system drop-ins, optional user file, optional user drop-ins,
/// `M80_*` environment variables, then `args_overrides`.
pub fn load_from_paths(
    args_overrides: HashMap<String, String>,
    paths: ConfigFilePaths,
) -> Result<EffectiveConfig, FcError> {
    let mut fields = defaults();

    // Layer 2: system config.
    if let Some(system_path) = paths.system {
        if system_path.exists() {
            let content = std::fs::read_to_string(&system_path)?;
            apply_toml_layer(&mut fields, &content, ConfigSource::SystemFile)
                .map_err(|e| FcError::Config(format!("system config: {e}")))?;
        }
    }

    // Layer 3: system config.d/*.toml in lexicographic order.
    if let Some(system_drop_in_dir) = paths.system_drop_in_dir {
        apply_drop_in_dir(
            &mut fields,
            &system_drop_in_dir,
            ConfigSource::SystemDropIn,
            "system config drop-in",
        )?;
    }

    // Layer 4: user config.
    if let Some(user_path) = paths.user {
        if user_path.exists() {
            let content = std::fs::read_to_string(&user_path)?;
            apply_toml_layer(&mut fields, &content, ConfigSource::UserFile)
                .map_err(|e| FcError::Config(format!("user config: {e}")))?;
        }
    }

    // Layer 5: user config.d/*.toml in lexicographic order.
    if let Some(user_drop_in_dir) = paths.user_drop_in_dir {
        apply_drop_in_dir(
            &mut fields,
            &user_drop_in_dir,
            ConfigSource::UserDropIn,
            "user config drop-in",
        )?;
    }

    // Layer 6: M80_* environment variables.
    let env_mapping: &[(&str, &str)] = &[
        ("M80_DEFAULT_PROFILE", field::DEFAULT_PROFILE),
        ("M80_MAX_CONCURRENT_VMS", field::MAX_CONCURRENT_VMS),
        ("M80_RUN_ROOT", field::RUN_ROOT),
        ("M80_JAIL_UID", field::JAIL_UID),
        ("M80_JAIL_GID", field::JAIL_GID),
        ("M80_CGROUP_MODE", field::CGROUP_MODE),
    ];
    for (env_key, field_name) in env_mapping {
        if let Ok(val) = std::env::var(env_key) {
            fields.insert(field_name.to_string(), (val, ConfigSource::Env));
        }
    }

    // Layer 7: caller-supplied flag overrides.
    for (k, v) in args_overrides {
        match fields.entry(k) {
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                entry.insert((v, ConfigSource::Flag));
            }
            std::collections::hash_map::Entry::Vacant(entry) => {
                return Err(FcError::Config(format!(
                    "unknown config override {:?}",
                    entry.key()
                )));
            }
        }
    }

    // Convert to EffectiveConfig.
    let mut effective_fields: Vec<EffectiveField> = fields
        .into_iter()
        .map(|(name, (value, source))| EffectiveField {
            name,
            value,
            source,
        })
        .collect();
    // Sort for deterministic output.
    effective_fields.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(EffectiveConfig {
        fields: effective_fields,
    })
}

fn apply_drop_in_dir(
    fields: &mut HashMap<String, (String, ConfigSource)>,
    dir: &Path,
    source: ConfigSource,
    label: &str,
) -> Result<(), FcError> {
    if !dir.exists() {
        return Ok(());
    }
    if !dir.is_dir() {
        return Err(FcError::Config(format!(
            "{label} path is not a directory: {}",
            dir.display()
        )));
    }

    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) == Some("toml") && path.is_file() {
            files.push(path);
        }
    }
    files.sort();

    for path in files {
        let content = std::fs::read_to_string(&path)?;
        apply_toml_layer(fields, &content, source)
            .map_err(|e| FcError::Config(format!("{label} {}: {e}", path.display())))?;
    }

    Ok(())
}

/// Parse recognized keys from `toml_text` and override `fields`.
fn apply_toml_layer(
    fields: &mut HashMap<String, (String, ConfigSource)>,
    toml_text: &str,
    source: ConfigSource,
) -> Result<(), String> {
    let table: toml::Value = toml::from_str(toml_text).map_err(|e| e.to_string())?;
    if let toml::Value::Table(map) = table {
        for (key, val) in map {
            if fields.contains_key(key.as_str()) {
                let s = match &val {
                    toml::Value::String(s) => s.clone(),
                    toml::Value::Integer(i) => i.to_string(),
                    toml::Value::Boolean(b) => b.to_string(),
                    other => format!("{other}"),
                };
                fields.insert(key, (s, source));
            } else {
                return Err(format!("unknown config key {key:?}"));
            }
        }
    } else {
        return Err("config root must be a TOML table".to_owned());
    }
    Ok(())
}

/// Best-effort home directory lookup without an external crate.
fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// Extract a typed [`BackendConfig`] from an [`EffectiveConfig`].
///
/// `discovery` must already be populated by the caller (from `m80-preflight`).
pub fn backend_config_from_effective(
    effective: &EffectiveConfig,
    discovery: m80_preflight::Discovery,
) -> Result<BackendConfig, FcError> {
    let get = |name: &str| -> Option<&str> {
        effective
            .fields
            .iter()
            .find(|f| f.name == name)
            .map(|f| f.value.as_str())
    };

    let max_concurrent_vms: u32 = get(field::MAX_CONCURRENT_VMS)
        .unwrap_or("8")
        .parse()
        .map_err(|e| FcError::Config(format!("max_concurrent_vms must be a u32: {e}")))?;

    let run_root: PathBuf = get(field::RUN_ROOT).unwrap_or("/var/run/m80").into();

    let jail_uid: u32 = get(field::JAIL_UID)
        .unwrap_or("3000")
        .parse()
        .map_err(|e| FcError::Config(format!("jail_uid must be a u32: {e}")))?;

    let jail_gid: u32 = get(field::JAIL_GID)
        .unwrap_or("3000")
        .parse()
        .map_err(|e| FcError::Config(format!("jail_gid must be a u32: {e}")))?;

    let cgroup_mode = match get(field::CGROUP_MODE).unwrap_or("unified-v2") {
        "unified-v2" => CgroupMode::UnifiedV2,
        "disabled" => CgroupMode::Disabled,
        other => {
            return Err(FcError::Config(format!(
                "unknown cgroup_mode {other:?}; expected \"unified-v2\" or \"disabled\""
            )));
        }
    };

    Ok(BackendConfig {
        discovery,
        max_concurrent_vms,
        run_root,
        jail_uid,
        jail_gid,
        cgroup_mode,
    })
}
