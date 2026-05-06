//! KVM-free configuration loading fixtures.
//!
//! These tests exercise the canonical loader through explicit config paths so
//! they do not read a developer machine's `/etc/m80/config.toml` or user config.

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

fn with_isolated_env(test: impl FnOnce(&TempDir, &TempDir)) {
    let _lock = env_lock().lock().unwrap();
    let _restore = EnvRestore::capture();
    for key in ENV_KEYS {
        std::env::remove_var(key);
    }

    let system = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    std::env::set_var("HOME", home.path());
    test(&system, &home);
}

fn paths(system: &TempDir, home: &TempDir) -> ConfigFilePaths {
    ConfigFilePaths {
        system: Some(system.path().join("config.toml")),
        system_drop_in_dir: Some(system.path().join("config.d")),
        user: Some(home.path().join(".config/m80/config.toml")),
        user_drop_in_dir: Some(home.path().join(".config/m80/config.d")),
    }
}

fn write_system_drop_in(system: &TempDir, name: &str, text: &str) {
    let config_dir = system.path().join("config.d");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::write(config_dir.join(name), text).unwrap();
}

fn write_user_config(home: &TempDir, text: &str) {
    let config_dir = home.path().join(".config/m80");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::write(config_dir.join("config.toml"), text).unwrap();
}

fn write_user_drop_in(home: &TempDir, name: &str, text: &str) {
    let config_dir = home.path().join(".config/m80/config.d");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::write(config_dir.join(name), text).unwrap();
}

fn field<'a>(effective: &'a EffectiveConfig, name: &str) -> &'a m80_firecracker::EffectiveField {
    effective
        .fields
        .iter()
        .find(|field| field.name == name)
        .unwrap_or_else(|| panic!("missing field {name}: {:?}", effective.fields))
}

#[test]
fn built_in_defaults_are_loaded_without_host_files() {
    with_isolated_env(|system, home| {
        let effective = load_config_from_paths(HashMap::new(), paths(system, home)).unwrap();

        assert_eq!(field(&effective, "max_concurrent_vms").value, "8");
        assert_eq!(
            field(&effective, "max_concurrent_vms").source,
            ConfigSource::Default
        );
        assert_eq!(field(&effective, "run_root").value, "/var/run/m80");
        assert_eq!(field(&effective, "default_profile").value, "env");
        assert_eq!(
            field(&effective, "default_profile").source,
            ConfigSource::Default
        );
        assert_eq!(field(&effective, "jail_uid").value, "3000");
        assert_eq!(field(&effective, "jail_gid").value, "3000");
        assert_eq!(field(&effective, "cgroup_mode").value, "unified-v2");
    });
}

#[test]
fn precedence_is_defaults_system_user_env_flags() {
    with_isolated_env(|system, home| {
        std::fs::write(
            system.path().join("config.toml"),
            "run_root = \"/system\"\nmax_concurrent_vms = 10\njail_uid = 1000\ndefault_profile = \"system\"\n",
        )
        .unwrap();
        write_user_config(
            home,
            "run_root = \"/user\"\nmax_concurrent_vms = 11\njail_gid = 2000\ndefault_profile = \"user\"\n",
        );
        std::env::set_var("M80_RUN_ROOT", "/env");
        std::env::set_var("M80_JAIL_UID", "3000");
        std::env::set_var("M80_DEFAULT_PROFILE", "env-profile");

        let mut flags = HashMap::new();
        flags.insert("max_concurrent_vms".to_owned(), "12".to_owned());
        flags.insert("default_profile".to_owned(), "flag-profile".to_owned());

        let effective = load_config_from_paths(flags, paths(system, home)).unwrap();

        assert_eq!(field(&effective, "run_root").value, "/env");
        assert_eq!(field(&effective, "run_root").source, ConfigSource::Env);
        assert_eq!(field(&effective, "max_concurrent_vms").value, "12");
        assert_eq!(
            field(&effective, "max_concurrent_vms").source,
            ConfigSource::Flag
        );
        assert_eq!(field(&effective, "jail_uid").value, "3000");
        assert_eq!(field(&effective, "jail_uid").source, ConfigSource::Env);
        assert_eq!(field(&effective, "jail_gid").value, "2000");
        assert_eq!(field(&effective, "jail_gid").source, ConfigSource::UserFile);
        assert_eq!(field(&effective, "default_profile").value, "flag-profile");
        assert_eq!(
            field(&effective, "default_profile").source,
            ConfigSource::Flag
        );
        assert_eq!(field(&effective, "cgroup_mode").value, "unified-v2");
        assert_eq!(
            field(&effective, "cgroup_mode").source,
            ConfigSource::Default
        );
    });
}

#[test]
fn user_file_overrides_system_file_per_key() {
    with_isolated_env(|system, home| {
        std::fs::write(
            system.path().join("config.toml"),
            "run_root = \"/system\"\nmax_concurrent_vms = 10\n",
        )
        .unwrap();
        write_user_config(home, "run_root = \"/user\"\n");

        let effective = load_config_from_paths(HashMap::new(), paths(system, home)).unwrap();

        assert_eq!(field(&effective, "run_root").value, "/user");
        assert_eq!(field(&effective, "run_root").source, ConfigSource::UserFile);
        assert_eq!(field(&effective, "max_concurrent_vms").value, "10");
        assert_eq!(
            field(&effective, "max_concurrent_vms").source,
            ConfigSource::SystemFile
        );
    });
}

#[test]
fn system_drop_ins_apply_after_system_file_in_lexicographic_order() {
    with_isolated_env(|system, home| {
        std::fs::write(
            system.path().join("config.toml"),
            "run_root = \"/system-file\"\nmax_concurrent_vms = 9\n",
        )
        .unwrap();
        write_system_drop_in(
            system,
            "20-last.toml",
            "run_root = \"/system-last\"\nmax_concurrent_vms = 20\n",
        );
        write_system_drop_in(
            system,
            "10-first.toml",
            "run_root = \"/system-first\"\nmax_concurrent_vms = 10\n",
        );
        std::fs::write(
            system.path().join("config.d/ignored.txt"),
            "run_root = \"/ignored\"\n",
        )
        .unwrap();

        let effective = load_config_from_paths(HashMap::new(), paths(system, home)).unwrap();

        assert_eq!(field(&effective, "run_root").value, "/system-last");
        assert_eq!(
            field(&effective, "run_root").source,
            ConfigSource::SystemDropIn
        );
        assert_eq!(field(&effective, "max_concurrent_vms").value, "20");
        assert_eq!(
            field(&effective, "max_concurrent_vms").source,
            ConfigSource::SystemDropIn
        );
    });
}

#[test]
fn user_drop_ins_override_system_drop_ins_and_user_file() {
    with_isolated_env(|system, home| {
        write_system_drop_in(system, "10-system.toml", "run_root = \"/system-drop\"\n");
        write_user_config(
            home,
            "run_root = \"/user-file\"\ndefault_profile = \"user-file-profile\"\n",
        );
        write_user_drop_in(
            home,
            "10-user.toml",
            "run_root = \"/user-drop\"\ndefault_profile = \"user-drop-profile\"\n",
        );

        let effective = load_config_from_paths(HashMap::new(), paths(system, home)).unwrap();

        assert_eq!(field(&effective, "run_root").value, "/user-drop");
        assert_eq!(
            field(&effective, "run_root").source,
            ConfigSource::UserDropIn
        );
        assert_eq!(
            field(&effective, "default_profile").value,
            "user-drop-profile"
        );
        assert_eq!(
            field(&effective, "default_profile").source,
            ConfigSource::UserDropIn
        );
    });
}

#[test]
fn env_and_flags_override_drop_ins() {
    with_isolated_env(|system, home| {
        write_system_drop_in(system, "10-system.toml", "run_root = \"/system-drop\"\n");
        write_user_drop_in(
            home,
            "10-user.toml",
            "run_root = \"/user-drop\"\ndefault_profile = \"user-drop-profile\"\n",
        );
        std::env::set_var("M80_RUN_ROOT", "/env-run");

        let mut flags = HashMap::new();
        flags.insert("default_profile".to_owned(), "flag-profile".to_owned());

        let effective = load_config_from_paths(flags, paths(system, home)).unwrap();

        assert_eq!(field(&effective, "run_root").value, "/env-run");
        assert_eq!(field(&effective, "run_root").source, ConfigSource::Env);
        assert_eq!(field(&effective, "default_profile").value, "flag-profile");
        assert_eq!(
            field(&effective, "default_profile").source,
            ConfigSource::Flag
        );
    });
}

#[test]
fn drop_in_unknown_key_fails_closed() {
    with_isolated_env(|system, home| {
        write_system_drop_in(system, "10-bad.toml", "not_a_key = true\n");

        let err = load_config_from_paths(HashMap::new(), paths(system, home)).unwrap_err();

        assert!(
            err.to_string().contains("system config drop-in"),
            "unexpected error: {err}"
        );
        assert!(
            err.to_string().contains("unknown config key"),
            "unexpected error: {err}"
        );
    });
}

#[test]
fn drop_in_parse_error_fails_closed() {
    with_isolated_env(|system, home| {
        write_user_drop_in(home, "10-bad.toml", "run_root = ");

        let err = load_config_from_paths(HashMap::new(), paths(system, home)).unwrap_err();

        assert!(
            err.to_string().contains("user config drop-in"),
            "unexpected error: {err}"
        );
    });
}

#[test]
fn drop_in_path_that_is_not_a_directory_fails_closed() {
    with_isolated_env(|system, home| {
        std::fs::write(system.path().join("config.d"), "not a directory").unwrap();

        let err = load_config_from_paths(HashMap::new(), paths(system, home)).unwrap_err();

        assert!(
            err.to_string().contains("not a directory"),
            "unexpected error: {err}"
        );
    });
}

#[test]
fn every_documented_env_key_maps_to_its_field() {
    with_isolated_env(|system, home| {
        std::env::set_var("M80_MAX_CONCURRENT_VMS", "31");
        std::env::set_var("M80_DEFAULT_PROFILE", "python");
        std::env::set_var("M80_RUN_ROOT", "/env-run");
        std::env::set_var("M80_JAIL_UID", "3100");
        std::env::set_var("M80_JAIL_GID", "3200");
        std::env::set_var("M80_CGROUP_MODE", "disabled");

        let effective = load_config_from_paths(HashMap::new(), paths(system, home)).unwrap();

        assert_eq!(field(&effective, "max_concurrent_vms").value, "31");
        assert_eq!(
            field(&effective, "max_concurrent_vms").source,
            ConfigSource::Env
        );
        assert_eq!(field(&effective, "run_root").value, "/env-run");
        assert_eq!(field(&effective, "run_root").source, ConfigSource::Env);
        assert_eq!(field(&effective, "default_profile").value, "python");
        assert_eq!(
            field(&effective, "default_profile").source,
            ConfigSource::Env
        );
        assert_eq!(field(&effective, "jail_uid").value, "3100");
        assert_eq!(field(&effective, "jail_uid").source, ConfigSource::Env);
        assert_eq!(field(&effective, "jail_gid").value, "3200");
        assert_eq!(field(&effective, "jail_gid").source, ConfigSource::Env);
        assert_eq!(field(&effective, "cgroup_mode").value, "disabled");
        assert_eq!(field(&effective, "cgroup_mode").source, ConfigSource::Env);
    });
}

#[test]
fn unknown_toml_key_fails_closed() {
    with_isolated_env(|system, home| {
        std::fs::write(system.path().join("config.toml"), "not_a_key = true\n").unwrap();

        let err = load_config_from_paths(HashMap::new(), paths(system, home)).unwrap_err();

        assert!(
            err.to_string().contains("unknown config key"),
            "unexpected error: {err}"
        );
    });
}

#[test]
fn unknown_flag_override_fails_closed() {
    with_isolated_env(|system, home| {
        let mut flags = HashMap::new();
        flags.insert("not_a_real_field".to_owned(), "value".to_owned());

        let err = load_config_from_paths(flags, paths(system, home)).unwrap_err();

        assert!(
            err.to_string().contains("unknown config override"),
            "unexpected error: {err}"
        );
    });
}
