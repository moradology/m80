use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use super::{
    discover_binaries, BinaryDiscoveryConfig, DEFAULT_FIRECRACKER_BIN, DEFAULT_JAILER_BIN,
    DEFAULT_JAILER_HARDEN_BIN, ENV_FIRECRACKER_BIN, ENV_FIRECRACKER_VERSION, ENV_JAILER_BIN,
    ENV_JAILER_HARDEN_BIN,
};
use crate::PreflightError;

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
    let jailer_harden = dir.path().join("m80-jailer-harden");

    write_executable(
        &firecracker,
        &format!("#!/bin/sh\nprintf 'Firecracker {version}\\n'\n"),
    );
    write_executable(&jailer, "#!/bin/sh\nexit 0\n");
    write_executable(&jailer_harden, "#!/bin/sh\nexit 0\n");

    let config = BinaryDiscoveryConfig {
        firecracker_bin: firecracker,
        jailer_bin: jailer,
        jailer_harden_bin: jailer_harden,
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
    let jailer_harden = dir.path().join("m80-jailer-harden");
    let _firecracker = EnvGuard::set(ENV_FIRECRACKER_BIN, &firecracker);
    let _jailer = EnvGuard::set(ENV_JAILER_BIN, &jailer);
    let _jailer_harden = EnvGuard::set(ENV_JAILER_HARDEN_BIN, &jailer_harden);
    let _version = EnvGuard::set_str(ENV_FIRECRACKER_VERSION, "v1.15.1");

    let config = BinaryDiscoveryConfig::from_env();

    assert_eq!(config.firecracker_bin, firecracker);
    assert_eq!(config.jailer_bin, jailer);
    assert_eq!(config.jailer_harden_bin, jailer_harden);
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
    let _jailer_harden = EnvGuard::remove(ENV_JAILER_HARDEN_BIN);
    let _version = EnvGuard::remove(ENV_FIRECRACKER_VERSION);

    let config = BinaryDiscoveryConfig::from_env();

    assert_eq!(config.firecracker_bin, Path::new(DEFAULT_FIRECRACKER_BIN));
    assert_eq!(config.jailer_bin, Path::new(DEFAULT_JAILER_BIN));
    assert_eq!(
        config.jailer_harden_bin,
        Path::new(DEFAULT_JAILER_HARDEN_BIN)
    );
    assert_eq!(config.expected_firecracker_version, None);
}

#[test]
fn discovery_returns_resolved_paths_and_probed_firecracker_version() {
    let (_dir, config) = fixture_config("v1.15.1");

    let discovery = discover_binaries(&config, None).unwrap();

    assert_eq!(discovery.firecracker_bin, config.firecracker_bin);
    assert_eq!(discovery.firecracker_version, "v1.15.1");
    assert_eq!(discovery.jailer_bin, config.jailer_bin);
    assert_eq!(discovery.jailer_harden_bin, config.jailer_harden_bin);
}

#[test]
fn cached_firecracker_version_skips_version_subprocess() {
    let (_dir, config) = fixture_config("v1.15.1");
    fs::write(
        &config.firecracker_bin,
        "#!/bin/sh\nprintf 'Firecracker v1.15.0\\n'\n",
    )
    .unwrap();

    let discovery = discover_binaries(&config, Some("v1.15.1")).unwrap();

    assert_eq!(discovery.firecracker_version, "v1.15.1");
}

#[test]
fn missing_firecracker_binary_fails_closed() {
    let dir = tempfile::tempdir().unwrap();
    let config = BinaryDiscoveryConfig {
        firecracker_bin: dir.path().join("missing-firecracker"),
        jailer_bin: dir.path().join("jailer"),
        jailer_harden_bin: dir.path().join("m80-jailer-harden"),
        expected_firecracker_version: Some("v1.15.1".to_owned()),
    };

    let err = discover_binaries(&config, None).unwrap_err();

    assert!(matches!(err, PreflightError::FirecrackerBinaryNotFound));
}

#[test]
fn missing_jailer_binary_fails_closed() {
    let (dir, mut config) = fixture_config("v1.15.1");
    config.jailer_bin = dir.path().join("missing-jailer");

    let err = discover_binaries(&config, None).unwrap_err();

    assert!(matches!(err, PreflightError::JailerBinaryNotFound));
}

#[test]
fn missing_jailer_hardening_wrapper_fails_closed() {
    let (dir, mut config) = fixture_config("v1.15.1");
    config.jailer_harden_bin = dir.path().join("missing-m80-jailer-harden");

    let err = discover_binaries(&config, None).unwrap_err();

    assert!(matches!(err, PreflightError::JailerHardenBinaryNotFound));
}

#[test]
fn firecracker_version_mismatch_fails_closed() {
    let (_dir, mut config) = fixture_config("v1.15.1");
    config.expected_firecracker_version = Some("v1.14.0".to_owned());

    let err = discover_binaries(&config, None).unwrap_err();

    match err {
        PreflightError::FirecrackerVersionMismatch { expected, actual } => {
            assert_eq!(expected, "v1.14.0");
            assert_eq!(actual, "v1.15.1");
        }
        other => panic!("expected version mismatch, got {other:?}"),
    }
}

#[test]
fn firecracker_cve_floor_rejects_known_affected_version() {
    let (_dir, config) = fixture_config("v1.15.0");

    let err = discover_binaries(&config, None).unwrap_err();

    match err {
        PreflightError::FirecrackerCveFloorViolation {
            cve_id,
            actual,
            fixed_versions,
        } => {
            assert_eq!(cve_id, "CVE-2026-5747");
            assert_eq!(actual, "v1.15.0");
            assert_eq!(fixed_versions, "v1.14.4 or v1.15.1");
        }
        other => panic!("expected CVE floor violation, got {other:?}"),
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
