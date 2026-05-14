use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use super::{
    discover_binaries, verify_host_binaries, BinaryDiscoveryConfig, DEFAULT_FIRECRACKER_BIN,
    DEFAULT_FIRECRACKER_SECCOMP_FILTER, DEFAULT_JAILER_BIN, DEFAULT_JAILER_HARDEN_BIN,
    ENV_FIRECRACKER_BIN, ENV_FIRECRACKER_SECCOMP_FILTER, ENV_FIRECRACKER_VERSION, ENV_JAILER_BIN,
    ENV_JAILER_HARDEN_BIN,
};
use crate::PreflightError;
use m80_image_manifest::{HostBinariesManifest, HostBinaryEntry, HostBinaryName};

fn write_executable(path: &Path, body: &str) {
    fs::write(path, body).unwrap();
    let mut permissions = fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).unwrap();
}

fn write_seccomp_filter(path: &Path) {
    fs::write(path, b"{\"seccomp_level\":2}\n").unwrap();
}

fn sha256_file(path: &Path) -> String {
    use sha2::{Digest, Sha256};
    let bytes = fs::read(path).unwrap();
    hex::encode(Sha256::digest(bytes))
}

#[cfg(target_os = "linux")]
fn system_root_owned_executable() -> &'static Path {
    for candidate in [
        "/usr/bin/dash",
        "/bin/dash",
        "/usr/bin/true",
        "/bin/true",
        "/usr/bin/env",
    ] {
        let path = Path::new(candidate);
        let Ok(meta) = fs::symlink_metadata(path) else {
            continue;
        };
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let mode = meta.permissions().mode() & 0o7777;
        if meta.is_file() && meta.uid() == 0 && meta.gid() == 0 && mode <= 0o755 {
            return path;
        }
    }
    panic!("expected a root-owned system executable for host-binary tests");
}

fn write_host_binary_manifest(
    path: &Path,
    firecracker: &Path,
    jailer: &Path,
    jailer_harden: &Path,
    m80: &Path,
    m80_cli: &Path,
) {
    HostBinariesManifest::new(vec![
        HostBinaryEntry {
            name: HostBinaryName::Firecracker,
            path: firecracker.to_path_buf(),
            sha256: sha256_file(firecracker),
        },
        HostBinaryEntry {
            name: HostBinaryName::Jailer,
            path: jailer.to_path_buf(),
            sha256: sha256_file(jailer),
        },
        HostBinaryEntry {
            name: HostBinaryName::M80,
            path: m80.to_path_buf(),
            sha256: sha256_file(m80),
        },
        HostBinaryEntry {
            name: HostBinaryName::M80Cli,
            path: m80_cli.to_path_buf(),
            sha256: sha256_file(m80_cli),
        },
        HostBinaryEntry {
            name: HostBinaryName::M80JailerHarden,
            path: jailer_harden.to_path_buf(),
            sha256: sha256_file(jailer_harden),
        },
    ])
    .write(path)
    .unwrap();
}

fn fixture_config(version: &str) -> (tempfile::TempDir, BinaryDiscoveryConfig) {
    let dir = tempfile::tempdir().unwrap();
    let firecracker = dir.path().join("firecracker");
    let jailer = dir.path().join("jailer");
    let jailer_harden = dir.path().join("m80-jailer-harden");
    let firecracker_seccomp_filter = dir.path().join("firecracker-seccomp-filter.json");

    write_executable(
        &firecracker,
        &format!("#!/bin/sh\nprintf 'Firecracker {version}\\n'\n"),
    );
    write_executable(&jailer, "#!/bin/sh\nexit 0\n");
    write_executable(&jailer_harden, "#!/bin/sh\nexit 0\n");
    write_seccomp_filter(&firecracker_seccomp_filter);

    let config = BinaryDiscoveryConfig {
        firecracker_bin: firecracker,
        firecracker_seccomp_filter,
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
    let seccomp_filter = dir.path().join("firecracker-seccomp-filter.json");
    let jailer = dir.path().join("jailer");
    let jailer_harden = dir.path().join("m80-jailer-harden");
    let _firecracker = EnvGuard::set(ENV_FIRECRACKER_BIN, &firecracker);
    let _seccomp_filter = EnvGuard::set(ENV_FIRECRACKER_SECCOMP_FILTER, &seccomp_filter);
    let _jailer = EnvGuard::set(ENV_JAILER_BIN, &jailer);
    let _jailer_harden = EnvGuard::set(ENV_JAILER_HARDEN_BIN, &jailer_harden);
    let _version = EnvGuard::set_str(ENV_FIRECRACKER_VERSION, "v1.15.1");

    let config = BinaryDiscoveryConfig::from_env();

    assert_eq!(config.firecracker_bin, firecracker);
    assert_eq!(config.firecracker_seccomp_filter, seccomp_filter);
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
    let _seccomp_filter = EnvGuard::remove(ENV_FIRECRACKER_SECCOMP_FILTER);
    let _jailer = EnvGuard::remove(ENV_JAILER_BIN);
    let _jailer_harden = EnvGuard::remove(ENV_JAILER_HARDEN_BIN);
    let _version = EnvGuard::remove(ENV_FIRECRACKER_VERSION);

    let config = BinaryDiscoveryConfig::from_env();

    assert_eq!(config.firecracker_bin, Path::new(DEFAULT_FIRECRACKER_BIN));
    assert_eq!(
        config.firecracker_seccomp_filter,
        Path::new(DEFAULT_FIRECRACKER_SECCOMP_FILTER)
    );
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
    assert_eq!(
        discovery.firecracker_seccomp_filter,
        config.firecracker_seccomp_filter
    );
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
        firecracker_seccomp_filter: dir.path().join("firecracker-seccomp-filter.json"),
        jailer_bin: dir.path().join("jailer"),
        jailer_harden_bin: dir.path().join("m80-jailer-harden"),
        expected_firecracker_version: Some("v1.15.1".to_owned()),
    };
    write_seccomp_filter(&config.firecracker_seccomp_filter);

    let err = discover_binaries(&config, None).unwrap_err();

    assert!(matches!(err, PreflightError::FirecrackerBinaryNotFound));
}

#[test]
fn relative_firecracker_binary_path_fails_closed() {
    let dir = tempfile::tempdir().unwrap();
    let config = BinaryDiscoveryConfig {
        firecracker_bin: Path::new("firecracker").to_path_buf(),
        firecracker_seccomp_filter: dir.path().join("firecracker-seccomp-filter.json"),
        jailer_bin: dir.path().join("jailer"),
        jailer_harden_bin: dir.path().join("m80-jailer-harden"),
        expected_firecracker_version: Some("v1.15.1".to_owned()),
    };
    write_seccomp_filter(&config.firecracker_seccomp_filter);

    let err = discover_binaries(&config, None).unwrap_err();

    match err {
        PreflightError::NonAbsolutePath { kind, path } => {
            assert_eq!(kind, "firecracker");
            assert_eq!(path, Path::new("firecracker"));
        }
        other => panic!("expected non-absolute firecracker path, got {other:?}"),
    }
}

#[test]
fn missing_firecracker_seccomp_filter_fails_closed() {
    let (dir, mut config) = fixture_config("v1.15.1");
    config.firecracker_seccomp_filter = dir.path().join("missing-seccomp-filter.json");

    let err = discover_binaries(&config, None).unwrap_err();

    match err {
        PreflightError::FirecrackerSeccompFilterNotFound { path } => {
            assert_eq!(path, config.firecracker_seccomp_filter);
        }
        other => panic!("expected missing seccomp filter, got {other:?}"),
    }
}

#[test]
fn empty_firecracker_seccomp_filter_fails_closed() {
    let (dir, mut config) = fixture_config("v1.15.1");
    config.firecracker_seccomp_filter = dir.path().join("empty-seccomp-filter.json");
    fs::write(&config.firecracker_seccomp_filter, b"").unwrap();

    let err = discover_binaries(&config, None).unwrap_err();

    match err {
        PreflightError::FirecrackerSeccompFilterEmpty { path } => {
            assert_eq!(path, config.firecracker_seccomp_filter);
        }
        other => panic!("expected empty seccomp filter, got {other:?}"),
    }
}

#[test]
fn relative_firecracker_seccomp_filter_path_fails_closed() {
    let (_dir, mut config) = fixture_config("v1.15.1");
    config.firecracker_seccomp_filter = Path::new("firecracker-seccomp-filter.json").to_path_buf();

    let err = discover_binaries(&config, None).unwrap_err();

    match err {
        PreflightError::NonAbsolutePath { kind, path } => {
            assert_eq!(kind, "firecracker seccomp filter");
            assert_eq!(path, Path::new("firecracker-seccomp-filter.json"));
        }
        other => panic!("expected non-absolute seccomp filter path, got {other:?}"),
    }
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

#[cfg(target_os = "linux")]
#[test]
fn host_binary_manifest_hash_mismatch_fails_closed() {
    let dir = tempfile::tempdir().unwrap();
    let system_binary = system_root_owned_executable();
    let manifest_path = dir.path().join("host-binaries.manifest.json");
    write_host_binary_manifest(
        &manifest_path,
        system_binary,
        system_binary,
        system_binary,
        system_binary,
        system_binary,
    );
    let mut manifest = HostBinariesManifest::read(&manifest_path).unwrap();
    manifest.binaries[0].sha256 = "0".repeat(64);
    manifest.write(&manifest_path).unwrap();
    let config = BinaryDiscoveryConfig {
        firecracker_bin: system_binary.to_path_buf(),
        firecracker_seccomp_filter: dir.path().join("firecracker-seccomp-filter.json"),
        jailer_bin: system_binary.to_path_buf(),
        jailer_harden_bin: system_binary.to_path_buf(),
        expected_firecracker_version: None,
    };
    write_seccomp_filter(&config.firecracker_seccomp_filter);

    let err = verify_host_binaries(&config, &manifest_path).unwrap_err();

    match err {
        PreflightError::BinaryHashMismatch { name, path, .. } => {
            assert_eq!(name, "firecracker");
            assert_eq!(path, system_binary);
        }
        other => panic!("expected binary hash mismatch, got {other:?}"),
    }
}

#[test]
fn host_binary_manifest_rejects_path_mismatch() {
    let (dir, config) = fixture_config("v1.15.1");
    let manifest_path = dir.path().join("host-binaries.manifest.json");
    let other_firecracker = dir.path().join("other-firecracker");
    write_executable(&other_firecracker, "#!/bin/sh\nexit 0\n");
    write_host_binary_manifest(
        &manifest_path,
        &other_firecracker,
        &config.jailer_bin,
        &config.jailer_harden_bin,
        &config.firecracker_bin,
        &config.firecracker_bin,
    );

    let err = verify_host_binaries(&config, &manifest_path).unwrap_err();

    match err {
        PreflightError::HostBinaryPathMismatch {
            name,
            expected,
            actual,
        } => {
            assert_eq!(name, "firecracker");
            assert_eq!(expected, config.firecracker_bin);
            assert_eq!(actual, dir.path().join("other-firecracker"));
        }
        other => panic!("expected host binary path mismatch, got {other:?}"),
    }
}

#[test]
fn host_binary_manifest_rejects_unsafe_permissions() {
    let (dir, config) = fixture_config("v1.15.1");
    let manifest_path = dir.path().join("host-binaries.manifest.json");
    write_host_binary_manifest(
        &manifest_path,
        &config.firecracker_bin,
        &config.jailer_bin,
        &config.jailer_harden_bin,
        &config.firecracker_bin,
        &config.firecracker_bin,
    );

    let err = verify_host_binaries(&config, &manifest_path).unwrap_err();

    match err {
        PreflightError::HostBinaryPermission { name, path, .. } => {
            assert_eq!(name, "firecracker");
            assert_eq!(path, config.firecracker_bin);
        }
        other => panic!("expected host binary permission rejection, got {other:?}"),
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
