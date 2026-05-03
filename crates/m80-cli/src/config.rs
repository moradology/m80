//! Configuration loading for `m80-cli`.
//!
//! Implements the documented precedence chain:
//! 1. Built-in defaults.
//! 2. `/etc/m80/config.toml` (system file).
//! 3. `~/.config/m80/config.toml` (user file).
//! 4. `M80_*` environment variables.
//! 5. CLI flag overrides (caller-supplied).
//!
//! The resolved values are assembled into a [`BackendConfig`] that
//! `Backend::new` accepts. Each field is also tagged with its winning
//! [`ConfigSource`] so `m80 config show` can report provenance.
//!
//! Behavior captures: beads `m80-v7t.1` (env-var schema),
//! `m80-v7t.2` (loading order).

use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::Context;

use m80_firecracker::{BackendConfig, CgroupMode, ConfigSource, EffectiveConfig, EffectiveField};
use m80_preflight::Discovery;

// =====================================================================
// Env-var keys (bead m80-v7t.1)
// =====================================================================
const ENV_RUN_ROOT: &str = "M80_RUN_ROOT";
const ENV_MAX_CONCURRENT_VMS: &str = "M80_MAX_CONCURRENT_VMS";
const ENV_JAIL_UID: &str = "M80_JAIL_UID";
const ENV_JAIL_GID: &str = "M80_JAIL_GID";
const ENV_CGROUP_MODE: &str = "M80_CGROUP_MODE";

// =====================================================================
// Built-in defaults
// =====================================================================
const DEFAULT_MAX_CONCURRENT_VMS: u32 = 4;
const DEFAULT_RUN_ROOT: &str = "/var/run/m80";
const DEFAULT_JAIL_UID: u32 = 10000;
const DEFAULT_JAIL_GID: u32 = 10000;
const DEFAULT_CGROUP_MODE: CgroupMode = CgroupMode::UnifiedV2;

// =====================================================================
// TOML config file schema (subset we care about)
// =====================================================================

/// Values parsed from a config.toml file (all optional — missing keys
/// fall through to the next layer).
#[derive(Debug, Default, serde::Deserialize)]
struct FileConfig {
    run_root: Option<String>,
    max_concurrent_vms: Option<u32>,
    jail_uid: Option<u32>,
    jail_gid: Option<u32>,
    cgroup_mode: Option<String>,
}

fn load_toml_file(path: &std::path::Path) -> anyhow::Result<Option<FileConfig>> {
    if !path.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("reading config file {}", path.display()))?;
    let cfg: FileConfig =
        toml::from_str(&text).with_context(|| format!("parsing config file {}", path.display()))?;
    Ok(Some(cfg))
}

fn parse_cgroup_mode(s: &str) -> anyhow::Result<CgroupMode> {
    match s {
        "unified_v2" | "unified-v2" => Ok(CgroupMode::UnifiedV2),
        "disabled" => Ok(CgroupMode::Disabled),
        other => anyhow::bail!(
            "unknown cgroup_mode '{}'; expected: unified_v2 | disabled",
            other
        ),
    }
}

// =====================================================================
// Resolved field value + source
// =====================================================================

struct Resolved<T> {
    value: T,
    source: ConfigSource,
}

impl<T> Resolved<T> {
    fn default(value: T) -> Self {
        Self {
            value,
            source: ConfigSource::Default,
        }
    }

    fn override_with(self, maybe: Option<T>, source: ConfigSource) -> Self {
        match maybe {
            Some(v) => Self { value: v, source },
            None => self,
        }
    }
}

// =====================================================================
// Public entry point
// =====================================================================

/// Load and merge all configuration sources into a [`BackendConfig`].
///
/// `flag_overrides` is a map of field-name → string-value from CLI flags;
/// the caller populates it when the user supplies explicit flags.
pub fn load(
    discovery: Discovery,
    flag_overrides: &HashMap<&str, String>,
) -> anyhow::Result<(BackendConfig, EffectiveConfig)> {
    // Layer 2: system file.
    let system_cfg = load_toml_file(std::path::Path::new("/etc/m80/config.toml"))
        .context("system config file")?
        .unwrap_or_default();

    // Layer 3: user file.
    let user_cfg = {
        let path = dirs_user_config();
        if let Some(p) = path {
            load_toml_file(&p)
                .context("user config file")?
                .unwrap_or_default()
        } else {
            FileConfig::default()
        }
    };

    // ---- run_root ----
    let run_root: Resolved<PathBuf> = Resolved::default(PathBuf::from(DEFAULT_RUN_ROOT))
        .override_with(
            system_cfg.run_root.as_deref().map(PathBuf::from),
            ConfigSource::SystemFile,
        )
        .override_with(
            user_cfg.run_root.as_deref().map(PathBuf::from),
            ConfigSource::UserFile,
        )
        .override_with(
            std::env::var(ENV_RUN_ROOT).ok().map(PathBuf::from),
            ConfigSource::Env,
        )
        .override_with(
            flag_overrides.get("run_root").map(PathBuf::from),
            ConfigSource::Flag,
        );

    // ---- max_concurrent_vms ----
    let max_vms: Resolved<u32> = Resolved::default(DEFAULT_MAX_CONCURRENT_VMS)
        .override_with(system_cfg.max_concurrent_vms, ConfigSource::SystemFile)
        .override_with(user_cfg.max_concurrent_vms, ConfigSource::UserFile)
        .override_with(
            std::env::var(ENV_MAX_CONCURRENT_VMS)
                .ok()
                .and_then(|s| s.parse().ok()),
            ConfigSource::Env,
        )
        .override_with(
            flag_overrides
                .get("max_concurrent_vms")
                .and_then(|s| s.parse().ok()),
            ConfigSource::Flag,
        );

    // ---- jail_uid ----
    let jail_uid: Resolved<u32> = Resolved::default(DEFAULT_JAIL_UID)
        .override_with(system_cfg.jail_uid, ConfigSource::SystemFile)
        .override_with(user_cfg.jail_uid, ConfigSource::UserFile)
        .override_with(
            std::env::var(ENV_JAIL_UID)
                .ok()
                .and_then(|s| s.parse().ok()),
            ConfigSource::Env,
        )
        .override_with(
            flag_overrides
                .get("jail_uid")
                .and_then(|s| s.parse().ok()),
            ConfigSource::Flag,
        );

    // ---- jail_gid ----
    let jail_gid: Resolved<u32> = Resolved::default(DEFAULT_JAIL_GID)
        .override_with(system_cfg.jail_gid, ConfigSource::SystemFile)
        .override_with(user_cfg.jail_gid, ConfigSource::UserFile)
        .override_with(
            std::env::var(ENV_JAIL_GID)
                .ok()
                .and_then(|s| s.parse().ok()),
            ConfigSource::Env,
        )
        .override_with(
            flag_overrides
                .get("jail_gid")
                .and_then(|s| s.parse().ok()),
            ConfigSource::Flag,
        );

    // ---- cgroup_mode ----
    let cgroup_sys = system_cfg
        .cgroup_mode
        .as_deref()
        .map(parse_cgroup_mode)
        .transpose()
        .context("system config cgroup_mode")?;
    let cgroup_user = user_cfg
        .cgroup_mode
        .as_deref()
        .map(parse_cgroup_mode)
        .transpose()
        .context("user config cgroup_mode")?;
    let cgroup_env = std::env::var(ENV_CGROUP_MODE)
        .ok()
        .map(|s| parse_cgroup_mode(&s))
        .transpose()
        .context("M80_CGROUP_MODE env var")?;
    let cgroup_flag = flag_overrides
        .get("cgroup_mode")
        .map(|s| parse_cgroup_mode(s))
        .transpose()
        .context("--cgroup-mode flag")?;

    let cgroup_mode: Resolved<CgroupMode> = Resolved::default(DEFAULT_CGROUP_MODE)
        .override_with(cgroup_sys, ConfigSource::SystemFile)
        .override_with(cgroup_user, ConfigSource::UserFile)
        .override_with(cgroup_env, ConfigSource::Env)
        .override_with(cgroup_flag, ConfigSource::Flag);

    // ---- Assemble EffectiveConfig for diagnostics ----
    let effective = EffectiveConfig {
        fields: vec![
            EffectiveField {
                name: "run_root".into(),
                value: run_root.value.display().to_string(),
                source: run_root.source,
            },
            EffectiveField {
                name: "max_concurrent_vms".into(),
                value: max_vms.value.to_string(),
                source: max_vms.source,
            },
            EffectiveField {
                name: "jail_uid".into(),
                value: jail_uid.value.to_string(),
                source: jail_uid.source,
            },
            EffectiveField {
                name: "jail_gid".into(),
                value: jail_gid.value.to_string(),
                source: jail_gid.source,
            },
            EffectiveField {
                name: "cgroup_mode".into(),
                value: format!("{:?}", cgroup_mode.value).to_ascii_lowercase(),
                source: cgroup_mode.source,
            },
        ],
    };

    let backend_config = BackendConfig {
        discovery,
        max_concurrent_vms: max_vms.value,
        run_root: run_root.value,
        jail_uid: jail_uid.value,
        jail_gid: jail_gid.value,
        cgroup_mode: cgroup_mode.value,
    };

    Ok((backend_config, effective))
}

/// Resolve `~/.config/m80/config.toml` without an external `dirs` crate.
fn dirs_user_config() -> Option<PathBuf> {
    // Honor XDG_CONFIG_HOME if set; otherwise fall back to $HOME/.config.
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config"))
        })?;
    Some(base.join("m80").join("config.toml"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_cgroup_mode_unified_v2() {
        assert_eq!(
            parse_cgroup_mode("unified_v2").unwrap(),
            CgroupMode::UnifiedV2
        );
        assert_eq!(
            parse_cgroup_mode("unified-v2").unwrap(),
            CgroupMode::UnifiedV2
        );
    }

    #[test]
    fn parse_cgroup_mode_disabled() {
        assert_eq!(
            parse_cgroup_mode("disabled").unwrap(),
            CgroupMode::Disabled
        );
    }

    #[test]
    fn parse_cgroup_mode_unknown_fails() {
        let err = parse_cgroup_mode("legacy").unwrap_err();
        assert!(err.to_string().contains("unknown cgroup_mode"), "{err}");
    }
}
