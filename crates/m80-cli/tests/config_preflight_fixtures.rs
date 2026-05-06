//! KVM-free config fixtures for the CLI facade.
//!
//! These tests pass explicit config paths and isolate M80_* values so they do
//! not depend on a developer machine's /etc or user config.

use std::collections::HashMap;
use std::ffi::OsString;
use std::sync::{Mutex, OnceLock};

use m80_firecracker::{load_config_from_paths, ConfigFilePaths, ConfigSource, EffectiveConfig};
use tempfile::TempDir;

const ENV_KEYS: &[&str] = &[
    "HOME",
    "M80_DEFAULT_PROFILE",
    "M80_MAX_CONCURRENT_VMS",
    "M80_RUN_ROOT",
    "M80_JAIL_UID",
    "M80_JAIL_GID",
    "M80_CGROUP_MODE",
];

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

struct EnvRestore {
    values: Vec<(&'static str, Option<OsString>)>,
}

impl EnvRestore {
    fn capture() -> Self {
        Self {
            values: ENV_KEYS
                .iter()
                .map(|key| (*key, std::env::var_os(key)))
                .collect(),
        }
    }
}

impl Drop for EnvRestore {
    fn drop(&mut self) {
        for (key, value) in &self.values {
            if let Some(value) = value {
                std::env::set_var(key, value);
            } else {
                std::env::remove_var(key);
            }
        }
    }
}

fn with_isolated_home(test: impl FnOnce(&TempDir)) {
    let _lock = env_lock().lock().unwrap();
    let _restore = EnvRestore::capture();
    for key in ENV_KEYS {
        std::env::remove_var(key);
    }

    let home = tempfile::tempdir().unwrap();
    std::env::set_var("HOME", home.path());
    test(&home);
}

fn isolated_paths(home: &TempDir) -> ConfigFilePaths {
    ConfigFilePaths {
        system: None,
        system_drop_in_dir: None,
        user: Some(home.path().join(".config/m80/config.toml")),
        user_drop_in_dir: Some(home.path().join(".config/m80/config.d")),
    }
}

fn field<'a>(effective: &'a EffectiveConfig, name: &str) -> &'a m80_firecracker::EffectiveField {
    effective
        .fields
        .iter()
        .find(|field| field.name == name)
        .unwrap_or_else(|| panic!("missing field {name}: {:?}", effective.fields))
}

#[test]
fn built_in_defaults_use_no_host_config_files() {
    with_isolated_home(|home| {
        let effective = load_config_from_paths(HashMap::new(), isolated_paths(home)).unwrap();
        let run_root = field(&effective, "run_root");
        let default_profile = field(&effective, "default_profile");
        let max_vms = field(&effective, "max_concurrent_vms");

        assert_eq!(run_root.value, "/var/run/m80");
        assert_eq!(run_root.source, ConfigSource::Default);
        assert_eq!(default_profile.value, "env");
        assert_eq!(default_profile.source, ConfigSource::Default);
        assert_eq!(max_vms.value, "8");
        assert_eq!(max_vms.source, ConfigSource::Default);
    });
}

#[test]
fn user_config_fixture_is_read_from_isolated_home() {
    with_isolated_home(|home| {
        let config_dir = home.path().join(".config/m80");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("config.toml"),
            "run_root = \"/tmp/m80-user\"\nmax_concurrent_vms = 11\n",
        )
        .unwrap();

        let effective = load_config_from_paths(HashMap::new(), isolated_paths(home)).unwrap();
        let run_root = field(&effective, "run_root");
        let max_vms = field(&effective, "max_concurrent_vms");

        assert_eq!(run_root.value, "/tmp/m80-user");
        assert_eq!(run_root.source, ConfigSource::UserFile);
        assert_eq!(max_vms.value, "11");
        assert_eq!(max_vms.source, ConfigSource::UserFile);
    });
}

#[test]
fn env_fixture_overrides_user_config() {
    with_isolated_home(|home| {
        let config_dir = home.path().join(".config/m80");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("config.toml"),
            "run_root = \"/tmp/m80-user\"\n",
        )
        .unwrap();
        std::env::set_var("M80_RUN_ROOT", "/tmp/m80-env");

        let effective = load_config_from_paths(HashMap::new(), isolated_paths(home)).unwrap();
        let run_root = field(&effective, "run_root");

        assert_eq!(run_root.value, "/tmp/m80-env");
        assert_eq!(run_root.source, ConfigSource::Env);
    });
}

#[test]
fn flag_fixture_overrides_env() {
    with_isolated_home(|home| {
        std::env::set_var("M80_RUN_ROOT", "/tmp/m80-env");
        let mut flags = HashMap::new();
        flags.insert("run_root".to_owned(), "/tmp/m80-flag".to_owned());

        let effective = load_config_from_paths(flags, isolated_paths(home)).unwrap();
        let run_root = field(&effective, "run_root");

        assert_eq!(run_root.value, "/tmp/m80-flag");
        assert_eq!(run_root.source, ConfigSource::Flag);
    });
}
