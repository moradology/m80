use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use m80_preflight::{
    discover_binaries, BinaryDiscoveryConfig, PreflightError, DEFAULT_FIRECRACKER_BIN,
    DEFAULT_JAILER_BIN, ENV_FIRECRACKER_BIN, ENV_FIRECRACKER_VERSION, ENV_JAILER_BIN,
};

fn write_executable(path: &Path, body: &str) {
    fs::write(path, body).unwrap();
    let mut permissions = fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).unwrap();
}

fn fixture_config(version: &str) -> (tempfile::TempDir, BinaryDiscoveryConfig) {
    let dir = tempfile::tempdir().unwrap();
    let firecracker = dir.path().join("firecracker");
    let jailer = dir.path().join("jailer");

    write_executable(
        &firecracker,
        &format!("#!/bin/sh\nprintf 'Firecracker {version}\\n'\n"),
    );
    write_executable(&jailer, "#!/bin/sh\nexit 0\n");

    let config = BinaryDiscoveryConfig {
        firecracker_bin: firecracker,
        jailer_bin: jailer,
        expected_firecracker_version: Some(version.to_owned()),
    };
    (dir, config)
}

#[test]
fn env_config_uses_exact_m80_keys_and_defaults() {
    let _lock = ENV_LOCK.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let firecracker = dir.path().join("firecracker");
    let jailer = dir.path().join("jailer");
    let _firecracker = EnvGuard::set(ENV_FIRECRACKER_BIN, &firecracker);
    let _jailer = EnvGuard::set(ENV_JAILER_BIN, &jailer);
    let _version = EnvGuard::set_str(ENV_FIRECRACKER_VERSION, "v1.15.1");

    let config = BinaryDiscoveryConfig::from_env();

    assert_eq!(config.firecracker_bin, firecracker);
    assert_eq!(config.jailer_bin, jailer);
    assert_eq!(
        config.expected_firecracker_version,
        Some("v1.15.1".to_owned())
    );
}

#[test]
fn env_config_defaults_to_opt_firecracker_paths_without_version_pin() {
    let _lock = ENV_LOCK.lock().unwrap();
    let _firecracker = EnvGuard::remove(ENV_FIRECRACKER_BIN);
    let _jailer = EnvGuard::remove(ENV_JAILER_BIN);
    let _version = EnvGuard::remove(ENV_FIRECRACKER_VERSION);

    let config = BinaryDiscoveryConfig::from_env();

    assert_eq!(config.firecracker_bin, Path::new(DEFAULT_FIRECRACKER_BIN));
    assert_eq!(config.jailer_bin, Path::new(DEFAULT_JAILER_BIN));
    assert_eq!(config.expected_firecracker_version, None);
}

#[test]
fn discovery_returns_resolved_paths_and_probed_firecracker_version() {
    let (_dir, config) = fixture_config("v1.15.1");

    let discovery = discover_binaries(&config).unwrap();

    assert_eq!(discovery.firecracker_bin, config.firecracker_bin);
    assert_eq!(discovery.firecracker_version, "v1.15.1");
    assert_eq!(discovery.jailer_bin, config.jailer_bin);
}

#[test]
fn missing_firecracker_binary_fails_closed() {
    let dir = tempfile::tempdir().unwrap();
    let config = BinaryDiscoveryConfig {
        firecracker_bin: dir.path().join("missing-firecracker"),
        jailer_bin: dir.path().join("jailer"),
        expected_firecracker_version: Some("v1.15.1".to_owned()),
    };

    let err = discover_binaries(&config).unwrap_err();

    assert!(matches!(err, PreflightError::FirecrackerBinaryNotFound));
}

#[test]
fn missing_jailer_binary_fails_closed() {
    let (dir, mut config) = fixture_config("v1.15.1");
    config.jailer_bin = dir.path().join("missing-jailer");

    let err = discover_binaries(&config).unwrap_err();

    assert!(matches!(err, PreflightError::JailerBinaryNotFound));
}

#[test]
fn firecracker_version_mismatch_fails_closed() {
    let (_dir, mut config) = fixture_config("v1.15.1");
    config.expected_firecracker_version = Some("v1.14.0".to_owned());

    let err = discover_binaries(&config).unwrap_err();

    match err {
        PreflightError::FirecrackerVersionMismatch { expected, actual } => {
            assert_eq!(expected, "v1.14.0");
            assert_eq!(actual, "v1.15.1");
        }
        other => panic!("expected version mismatch, got {other:?}"),
    }
}

static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct EnvGuard {
    key: &'static str,
    old: Option<std::ffi::OsString>,
}

impl EnvGuard {
    fn set(key: &'static str, value: &Path) -> Self {
        let old = std::env::var_os(key);
        std::env::set_var(key, value);
        Self { key, old }
    }

    fn set_str(key: &'static str, value: &str) -> Self {
        let old = std::env::var_os(key);
        std::env::set_var(key, value);
        Self { key, old }
    }

    fn remove(key: &'static str) -> Self {
        let old = std::env::var_os(key);
        std::env::remove_var(key);
        Self { key, old }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        if let Some(old) = &self.old {
            std::env::set_var(self.key, old);
        } else {
            std::env::remove_var(self.key);
        }
    }
}
