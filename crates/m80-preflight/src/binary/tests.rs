use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command as StdCommand;

use super::{
    discover_binaries, generate_host_binaries_manifest, verify_host_binaries, BinaryDiscovery,
    BinaryDiscoveryConfig, HostBinariesManifestConfig, DEFAULT_FIRECRACKER_BIN,
    DEFAULT_FIRECRACKER_SECCOMP_FILTER, DEFAULT_JAILER_BIN, DEFAULT_JAILER_HARDEN_BIN,
    DEFAULT_NET_HELPER_BIN, ENV_FIRECRACKER_BIN, ENV_FIRECRACKER_SECCOMP_FILTER,
    ENV_FIRECRACKER_VERSION, ENV_JAILER_BIN, ENV_JAILER_HARDEN_BIN, ENV_NET_HELPER_BIN,
};
use crate::{LaunchPath, PreflightError};
use m80_image_manifest::{
    ConditionalHostBinaryEntry, HostBinariesManifest, HostBinaryAbsentWhen, HostBinaryEntry,
    HostBinaryName, HostLaunchMaterialEntry, HostLaunchMaterialName,
};

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
        "/usr/bin/gnutrue",
        "/bin/gnutrue",
        "/usr/bin/true",
        "/bin/true",
        "/usr/lib/cargo/bin/coreutils/env",
        "/usr/bin/env",
        "/usr/bin/dash",
        "/bin/dash",
    ] {
        let path = Path::new(candidate);
        let Ok(meta) = fs::symlink_metadata(path) else {
            continue;
        };
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let mode = meta.permissions().mode() & 0o7777;
        if meta.is_file() && meta.uid() == 0 && meta.gid() == 0 && mode <= 0o755 {
            let Ok(output) = StdCommand::new(path).arg("--version").output() else {
                continue;
            };
            if output.status.success() {
                return path;
            }
        }
    }
    panic!("expected a root-owned system executable with --version for host-binary tests");
}

#[cfg(target_os = "linux")]
fn system_root_owned_non_executable() -> &'static Path {
    for candidate in ["/etc/hosts", "/etc/passwd", "/etc/group"] {
        let path = Path::new(candidate);
        let Ok(meta) = fs::symlink_metadata(path) else {
            continue;
        };
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let mode = meta.permissions().mode() & 0o7777;
        if meta.is_file() && meta.uid() == 0 && meta.gid() == 0 && mode & 0o111 == 0 {
            return path;
        }
    }
    panic!("expected a root-owned non-executable system file for host-binary tests");
}

fn write_host_binary_manifest(
    path: &Path,
    firecracker: &Path,
    firecracker_seccomp_filter: &Path,
    jailer: &Path,
    jailer_harden: &Path,
    net_helper: &Path,
    m80: &Path,
) {
    let firecracker_version = "v1.15.1".to_owned();
    let jailer_version = "v1.15.1".to_owned();
    let helper_version = binary_version_stdout(jailer_harden);
    let net_helper_version = binary_version_stdout(net_helper);
    let m80_version = binary_version_stdout(m80);
    HostBinariesManifest::new(
        vec![
            HostBinaryEntry {
                name: HostBinaryName::Firecracker,
                path: firecracker.to_path_buf(),
                sha256: sha256_file(firecracker),
                version: firecracker_version.clone(),
            },
            HostBinaryEntry {
                name: HostBinaryName::Jailer,
                path: jailer.to_path_buf(),
                sha256: sha256_file(jailer),
                version: jailer_version,
            },
            HostBinaryEntry {
                name: HostBinaryName::M80,
                path: m80.to_path_buf(),
                sha256: sha256_file(m80),
                version: m80_version,
            },
            HostBinaryEntry {
                name: HostBinaryName::M80JailerHarden,
                path: jailer_harden.to_path_buf(),
                sha256: sha256_file(jailer_harden),
                version: helper_version,
            },
            HostBinaryEntry {
                name: HostBinaryName::M80NetHelper,
                path: net_helper.to_path_buf(),
                sha256: sha256_file(net_helper),
                version: net_helper_version,
            },
        ],
        vec![HostLaunchMaterialEntry {
            name: HostLaunchMaterialName::FirecrackerSeccompFilter,
            path: firecracker_seccomp_filter.to_path_buf(),
            sha256: sha256_file(firecracker_seccomp_filter),
            version: firecracker_version,
        }],
    )
    .write(path)
    .unwrap();
}

fn binary_version_stdout(path: &Path) -> String {
    let output = StdCommand::new(path).arg("--version").output().unwrap();
    if !output.status.success() {
        return "test-version".to_owned();
    }
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn fixture_discovery(config: &BinaryDiscoveryConfig, version: &str) -> BinaryDiscovery {
    BinaryDiscovery {
        firecracker_bin: config.firecracker_bin.clone(),
        firecracker_seccomp_filter: config.firecracker_seccomp_filter.clone(),
        firecracker_version: version.to_owned(),
        jailer_bin: config.jailer_bin.clone(),
        jailer_version: version.to_owned(),
        jailer_harden_bin: config.jailer_harden_bin.clone(),
        net_helper_bin: config.net_helper_bin.clone(),
    }
}

fn verify_fixture_host_binaries(
    config: &BinaryDiscoveryConfig,
    manifest_path: &Path,
) -> Result<(), PreflightError> {
    let discovery = fixture_discovery(config, "v1.15.1");
    verify_host_binaries(config, &discovery, manifest_path, LaunchPath::Wrapper)
}

fn verify_fixture_host_binaries_for_launch_path(
    config: &BinaryDiscoveryConfig,
    manifest_path: &Path,
    launch_path: LaunchPath,
) -> Result<(), PreflightError> {
    let discovery = fixture_discovery(config, "v1.15.1");
    verify_host_binaries(config, &discovery, manifest_path, launch_path)
}

fn fixture_config(version: &str) -> (tempfile::TempDir, BinaryDiscoveryConfig) {
    let dir = tempfile::tempdir().unwrap();
    let firecracker = dir.path().join("firecracker");
    let jailer = dir.path().join("jailer");
    let jailer_harden = dir.path().join("m80-jailer-harden");
    let net_helper = dir.path().join("m80-net-helper");
    let firecracker_seccomp_filter = dir.path().join("firecracker-seccomp-filter.bin");

    write_executable(
        &firecracker,
        &format!("#!/bin/sh\nprintf 'Firecracker {version}\\n'\n"),
    );
    write_executable(
        &jailer,
        &format!("#!/bin/sh\nprintf 'Jailer {version}\\n'\n"),
    );
    write_executable(
        &jailer_harden,
        "#!/bin/sh\nif [ \"$1\" = --version ]; then printf 'm80-jailer-harden 0.1.0\\n'; fi\n",
    );
    write_executable(
        &net_helper,
        "#!/bin/sh\nif [ \"$1\" = --version ]; then printf 'm80-net-helper 0.1.0\\n'; fi\n",
    );
    write_seccomp_filter(&firecracker_seccomp_filter);

    let config = BinaryDiscoveryConfig {
        firecracker_bin: firecracker,
        firecracker_seccomp_filter,
        jailer_bin: jailer,
        jailer_harden_bin: jailer_harden,
        net_helper_bin: net_helper,
        expected_firecracker_version: Some(version.to_owned()),
    };
    (dir, config)
}

#[test]
fn env_config_uses_exact_m80_keys_and_defaults() {
    let _lock = ENV_LOCK.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let firecracker = dir.path().join("firecracker");
    let seccomp_filter = dir.path().join("firecracker-seccomp-filter.bin");
    let jailer = dir.path().join("jailer");
    let jailer_harden = dir.path().join("m80-jailer-harden");
    let net_helper = dir.path().join("m80-net-helper");
    let _firecracker = EnvGuard::set(ENV_FIRECRACKER_BIN, &firecracker);
    let _seccomp_filter = EnvGuard::set(ENV_FIRECRACKER_SECCOMP_FILTER, &seccomp_filter);
    let _jailer = EnvGuard::set(ENV_JAILER_BIN, &jailer);
    let _jailer_harden = EnvGuard::set(ENV_JAILER_HARDEN_BIN, &jailer_harden);
    let _net_helper = EnvGuard::set(ENV_NET_HELPER_BIN, &net_helper);
    let _version = EnvGuard::set_str(ENV_FIRECRACKER_VERSION, "v1.15.1");

    let config = BinaryDiscoveryConfig::from_env();

    assert_eq!(config.firecracker_bin, firecracker);
    assert_eq!(config.firecracker_seccomp_filter, seccomp_filter);
    assert_eq!(config.jailer_bin, jailer);
    assert_eq!(config.jailer_harden_bin, jailer_harden);
    assert_eq!(config.net_helper_bin, net_helper);
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
    let _net_helper = EnvGuard::remove(ENV_NET_HELPER_BIN);
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
    assert_eq!(config.net_helper_bin, Path::new(DEFAULT_NET_HELPER_BIN));
    assert_eq!(config.expected_firecracker_version, None);
}

#[test]
fn discovery_returns_resolved_paths_and_probed_firecracker_version() {
    let (_dir, config) = fixture_config("v1.15.1");

    let discovery = discover_binaries(&config, None, None, LaunchPath::Wrapper).unwrap();

    assert_eq!(discovery.firecracker_bin, config.firecracker_bin);
    assert_eq!(
        discovery.firecracker_seccomp_filter,
        config.firecracker_seccomp_filter
    );
    assert_eq!(discovery.firecracker_version, "v1.15.1");
    assert_eq!(discovery.jailer_bin, config.jailer_bin);
    assert_eq!(discovery.jailer_version, "v1.15.1");
    assert_eq!(discovery.jailer_harden_bin, config.jailer_harden_bin);
    assert_eq!(discovery.net_helper_bin, config.net_helper_bin);
}

#[test]
fn cached_train_versions_skip_version_subprocesses() {
    let (_dir, config) = fixture_config("v1.15.1");
    fs::write(
        &config.firecracker_bin,
        "#!/bin/sh\nprintf 'Firecracker v1.15.0\\n'\n",
    )
    .unwrap();

    let discovery = discover_binaries(
        &config,
        Some("v1.15.1"),
        Some("v1.15.1"),
        LaunchPath::Wrapper,
    )
    .unwrap();

    assert_eq!(discovery.firecracker_version, "v1.15.1");
    assert_eq!(discovery.jailer_version, "v1.15.1");
}

#[test]
fn missing_firecracker_binary_fails_closed() {
    let dir = tempfile::tempdir().unwrap();
    let config = BinaryDiscoveryConfig {
        firecracker_bin: dir.path().join("missing-firecracker"),
        firecracker_seccomp_filter: dir.path().join("firecracker-seccomp-filter.bin"),
        jailer_bin: dir.path().join("jailer"),
        jailer_harden_bin: dir.path().join("m80-jailer-harden"),
        net_helper_bin: dir.path().join("m80-net-helper"),
        expected_firecracker_version: Some("v1.15.1".to_owned()),
    };
    write_seccomp_filter(&config.firecracker_seccomp_filter);

    let err = discover_binaries(&config, None, None, LaunchPath::Wrapper).unwrap_err();

    match err {
        PreflightError::FirecrackerBinaryNotFound { path } => {
            assert_eq!(path, config.firecracker_bin);
        }
        other => panic!("expected missing firecracker binary, got {other:?}"),
    }
}

#[test]
fn relative_firecracker_binary_path_fails_closed() {
    let dir = tempfile::tempdir().unwrap();
    let config = BinaryDiscoveryConfig {
        firecracker_bin: Path::new("firecracker").to_path_buf(),
        firecracker_seccomp_filter: dir.path().join("firecracker-seccomp-filter.bin"),
        jailer_bin: dir.path().join("jailer"),
        jailer_harden_bin: dir.path().join("m80-jailer-harden"),
        net_helper_bin: dir.path().join("m80-net-helper"),
        expected_firecracker_version: Some("v1.15.1".to_owned()),
    };
    write_seccomp_filter(&config.firecracker_seccomp_filter);

    let err = discover_binaries(&config, None, None, LaunchPath::Wrapper).unwrap_err();

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

    let err = discover_binaries(&config, None, None, LaunchPath::Wrapper).unwrap_err();

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

    let err = discover_binaries(&config, None, None, LaunchPath::Wrapper).unwrap_err();

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
    config.firecracker_seccomp_filter = Path::new("firecracker-seccomp-filter.bin").to_path_buf();

    let err = discover_binaries(&config, None, None, LaunchPath::Wrapper).unwrap_err();

    match err {
        PreflightError::NonAbsolutePath { kind, path } => {
            assert_eq!(kind, "firecracker seccomp filter");
            assert_eq!(path, Path::new("firecracker-seccomp-filter.bin"));
        }
        other => panic!("expected non-absolute seccomp filter path, got {other:?}"),
    }
}

#[test]
fn missing_jailer_binary_fails_closed() {
    let (dir, mut config) = fixture_config("v1.15.1");
    config.jailer_bin = dir.path().join("missing-jailer");

    let err = discover_binaries(&config, None, None, LaunchPath::Wrapper).unwrap_err();

    match err {
        PreflightError::JailerBinaryNotFound { path } => {
            assert_eq!(path, config.jailer_bin);
        }
        other => panic!("expected missing jailer binary, got {other:?}"),
    }
}

#[test]
fn missing_jailer_hardening_wrapper_fails_closed() {
    let (dir, mut config) = fixture_config("v1.15.1");
    config.jailer_harden_bin = dir.path().join("missing-m80-jailer-harden");

    let err = discover_binaries(&config, None, None, LaunchPath::Wrapper).unwrap_err();

    match err {
        PreflightError::JailerHardenBinaryNotFound { path } => {
            assert_eq!(path, config.jailer_harden_bin);
        }
        other => panic!("expected missing jailer hardening wrapper, got {other:?}"),
    }
}

#[test]
fn missing_jailer_hardening_wrapper_is_allowed_for_systemd_discovery() {
    let (dir, mut config) = fixture_config("v1.15.1");
    config.jailer_harden_bin = dir.path().join("missing-m80-jailer-harden");

    let discovery = discover_binaries(&config, None, None, LaunchPath::Systemd).unwrap();

    assert_eq!(discovery.jailer_harden_bin, config.jailer_harden_bin);
}

#[test]
fn missing_network_helper_fails_closed() {
    let (dir, mut config) = fixture_config("v1.15.1");
    config.net_helper_bin = dir.path().join("missing-m80-net-helper");

    let err = discover_binaries(&config, None, None, LaunchPath::Wrapper).unwrap_err();

    match err {
        PreflightError::NetHelperBinaryNotFound { path } => {
            assert_eq!(path, config.net_helper_bin);
        }
        other => panic!("expected missing network helper, got {other:?}"),
    }
}

#[test]
fn firecracker_version_mismatch_fails_closed() {
    let (_dir, mut config) = fixture_config("v1.15.1");
    config.expected_firecracker_version = Some("v1.14.0".to_owned());

    let err = discover_binaries(&config, None, None, LaunchPath::Wrapper).unwrap_err();

    match err {
        PreflightError::FirecrackerVersionMismatch {
            expected,
            actual,
            policy_source,
        } => {
            assert_eq!(expected, "v1.14.0");
            assert_eq!(actual, "v1.15.1");
            assert_eq!(
                policy_source,
                "crates/m80-preflight/src/firecracker_train.rs"
            );
        }
        other => panic!("expected version mismatch, got {other:?}"),
    }
}

#[test]
fn jailer_version_mismatch_fails_closed() {
    let (_dir, config) = fixture_config("v1.15.1");
    fs::write(
        &config.jailer_bin,
        "#!/bin/sh\nprintf 'Jailer v1.14.4\\n'\n",
    )
    .unwrap();

    let err = discover_binaries(&config, None, None, LaunchPath::Wrapper).unwrap_err();

    match err {
        PreflightError::JailerVersionMismatch {
            expected,
            actual,
            policy_source,
        } => {
            assert_eq!(expected, "v1.15.1");
            assert_eq!(actual, "v1.14.4");
            assert_eq!(
                policy_source,
                "crates/m80-preflight/src/firecracker_train.rs"
            );
        }
        other => panic!("expected jailer version mismatch, got {other:?}"),
    }
}

#[test]
fn firecracker_cve_floor_rejects_known_affected_version() {
    let (_dir, config) = fixture_config("v1.15.0");

    let err = discover_binaries(&config, None, None, LaunchPath::Wrapper).unwrap_err();

    match err {
        PreflightError::FirecrackerCveFloorViolation {
            cve_id,
            actual,
            expected,
            policy_source,
        } => {
            assert_eq!(cve_id, "CVE-2026-5747");
            assert_eq!(actual, "v1.15.0");
            assert_eq!(expected, "v1.14.4 or v1.15.1");
            assert_eq!(policy_source, "crates/m80-preflight/src/cve_floor.rs");
        }
        other => panic!("expected CVE floor violation, got {other:?}"),
    }
}

#[test]
fn host_binaries_manifest_generator_records_final_paths_hashes_and_versions() {
    let (dir, config) = fixture_config("v1.15.1");
    let m80 = dir.path().join("m80");
    write_executable(
        &m80,
        "#!/bin/sh\nif [ \"$1\" = --version ]; then printf 'm80 0.1.0\\n'; fi\n",
    );
    let generator = HostBinariesManifestConfig {
        firecracker_bin: config.firecracker_bin.clone(),
        firecracker_seccomp_filter: config.firecracker_seccomp_filter.clone(),
        jailer_bin: config.jailer_bin.clone(),
        jailer_harden_bin: config.jailer_harden_bin.clone(),
        include_jailer_harden: true,
        net_helper_bin: config.net_helper_bin.clone(),
        m80_bin: m80.clone(),
        expected_firecracker_version: config.expected_firecracker_version.clone(),
    };

    let manifest = generate_host_binaries_manifest(&generator).unwrap();

    assert_eq!(
        manifest.schema_version(),
        m80_image_manifest::HOST_BINARIES_SCHEMA_VERSION
    );
    assert_eq!(manifest.binaries.len(), 5);
    assert!(manifest.conditional_binaries.is_empty());
    assert_eq!(manifest.launch_material.len(), 1);
    let firecracker = manifest
        .binaries
        .iter()
        .find(|entry| entry.name == HostBinaryName::Firecracker)
        .unwrap();
    assert_eq!(firecracker.path, config.firecracker_bin);
    assert_eq!(firecracker.sha256, sha256_file(&firecracker.path));
    assert_eq!(firecracker.version, "v1.15.1");
    let m80_entry = manifest
        .binaries
        .iter()
        .find(|entry| entry.name == HostBinaryName::M80)
        .unwrap();
    assert_eq!(m80_entry.path, m80);
    assert_eq!(m80_entry.sha256, sha256_file(&m80_entry.path));
    assert_eq!(m80_entry.version, "m80 0.1.0");
    let seccomp = &manifest.launch_material[0];
    assert_eq!(
        seccomp.name,
        HostLaunchMaterialName::FirecrackerSeccompFilter
    );
    assert_eq!(seccomp.path, config.firecracker_seccomp_filter);
    assert_eq!(seccomp.sha256, sha256_file(&seccomp.path));
    assert_eq!(seccomp.version, "v1.15.1");
}

#[test]
fn host_binaries_manifest_generator_can_omit_systemd_wrapper() {
    let (dir, config) = fixture_config("v1.15.1");
    let m80 = dir.path().join("m80");
    write_executable(
        &m80,
        "#!/bin/sh\nif [ \"$1\" = --version ]; then printf 'm80 0.1.0\\n'; fi\n",
    );
    let generator = HostBinariesManifestConfig {
        firecracker_bin: config.firecracker_bin.clone(),
        firecracker_seccomp_filter: config.firecracker_seccomp_filter.clone(),
        jailer_bin: config.jailer_bin.clone(),
        jailer_harden_bin: dir.path().join("missing-m80-jailer-harden"),
        include_jailer_harden: false,
        net_helper_bin: config.net_helper_bin.clone(),
        m80_bin: m80,
        expected_firecracker_version: config.expected_firecracker_version.clone(),
    };

    let manifest = generate_host_binaries_manifest(&generator).unwrap();

    assert!(!manifest
        .binaries
        .iter()
        .any(|entry| entry.name == HostBinaryName::M80JailerHarden));
    assert_eq!(
        manifest.conditional_binaries,
        vec![ConditionalHostBinaryEntry {
            name: HostBinaryName::M80JailerHarden,
            absent_when: HostBinaryAbsentWhen::SystemdPathChosen,
        }]
    );
}

#[cfg(target_os = "linux")]
#[test]
fn host_binary_manifest_exact_match_succeeds() {
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
        system_binary,
    );
    let config = BinaryDiscoveryConfig {
        firecracker_bin: system_binary.to_path_buf(),
        firecracker_seccomp_filter: system_binary.to_path_buf(),
        jailer_bin: system_binary.to_path_buf(),
        jailer_harden_bin: system_binary.to_path_buf(),
        net_helper_bin: system_binary.to_path_buf(),
        expected_firecracker_version: None,
    };

    verify_fixture_host_binaries(&config, &manifest_path).unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn host_binary_manifest_allows_absent_wrapper_only_for_systemd_path() {
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
        system_binary,
    );
    let mut manifest = HostBinariesManifest::read(&manifest_path).unwrap();
    manifest
        .binaries
        .retain(|entry| entry.name != HostBinaryName::M80JailerHarden);
    manifest.conditional_binaries = vec![ConditionalHostBinaryEntry {
        name: HostBinaryName::M80JailerHarden,
        absent_when: HostBinaryAbsentWhen::SystemdPathChosen,
    }];
    manifest.write(&manifest_path).unwrap();
    let config = BinaryDiscoveryConfig {
        firecracker_bin: system_binary.to_path_buf(),
        firecracker_seccomp_filter: system_binary.to_path_buf(),
        jailer_bin: system_binary.to_path_buf(),
        jailer_harden_bin: dir.path().join("missing-m80-jailer-harden"),
        net_helper_bin: system_binary.to_path_buf(),
        expected_firecracker_version: None,
    };

    verify_fixture_host_binaries_for_launch_path(&config, &manifest_path, LaunchPath::Systemd)
        .unwrap();
    let err =
        verify_fixture_host_binaries_for_launch_path(&config, &manifest_path, LaunchPath::Wrapper)
            .unwrap_err();

    match err {
        PreflightError::HostBinaryMissing { name } => {
            assert_eq!(name, "m80_jailer_harden");
        }
        other => panic!("expected wrapper missing for wrapper path, got {other:?}"),
    }
}

#[cfg(target_os = "linux")]
#[test]
fn host_binary_manifest_without_conditional_wrapper_fails_for_systemd_path() {
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
        system_binary,
    );
    let mut manifest = HostBinariesManifest::read(&manifest_path).unwrap();
    manifest
        .binaries
        .retain(|entry| entry.name != HostBinaryName::M80JailerHarden);
    manifest.write(&manifest_path).unwrap();
    let config = BinaryDiscoveryConfig {
        firecracker_bin: system_binary.to_path_buf(),
        firecracker_seccomp_filter: system_binary.to_path_buf(),
        jailer_bin: system_binary.to_path_buf(),
        jailer_harden_bin: dir.path().join("missing-m80-jailer-harden"),
        net_helper_bin: system_binary.to_path_buf(),
        expected_firecracker_version: None,
    };

    let err =
        verify_fixture_host_binaries_for_launch_path(&config, &manifest_path, LaunchPath::Systemd)
            .unwrap_err();

    match err {
        PreflightError::HostBinaryMissing { name } => {
            assert_eq!(name, "m80_jailer_harden");
        }
        other => panic!("expected wrapper missing without conditional, got {other:?}"),
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
        system_binary,
    );
    let mut manifest = HostBinariesManifest::read(&manifest_path).unwrap();
    manifest.binaries[0].sha256 = "0".repeat(64);
    manifest.write(&manifest_path).unwrap();
    let config = BinaryDiscoveryConfig {
        firecracker_bin: system_binary.to_path_buf(),
        firecracker_seccomp_filter: system_binary.to_path_buf(),
        jailer_bin: system_binary.to_path_buf(),
        jailer_harden_bin: system_binary.to_path_buf(),
        net_helper_bin: system_binary.to_path_buf(),
        expected_firecracker_version: None,
    };

    let err = verify_fixture_host_binaries(&config, &manifest_path).unwrap_err();

    match err {
        PreflightError::BinaryHashMismatch { name, path, .. } => {
            assert_eq!(name, "firecracker");
            assert_eq!(path, system_binary);
        }
        other => panic!("expected binary hash mismatch, got {other:?}"),
    }
}

#[cfg(target_os = "linux")]
#[test]
fn host_binary_manifest_helper_hash_mismatch_fails_closed() {
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
        system_binary,
    );
    let mut manifest = HostBinariesManifest::read(&manifest_path).unwrap();
    let helper = manifest
        .binaries
        .iter_mut()
        .find(|entry| entry.name == HostBinaryName::M80NetHelper)
        .unwrap();
    helper.sha256 = "0".repeat(64);
    manifest.write(&manifest_path).unwrap();
    let config = BinaryDiscoveryConfig {
        firecracker_bin: system_binary.to_path_buf(),
        firecracker_seccomp_filter: system_binary.to_path_buf(),
        jailer_bin: system_binary.to_path_buf(),
        jailer_harden_bin: system_binary.to_path_buf(),
        net_helper_bin: system_binary.to_path_buf(),
        expected_firecracker_version: None,
    };

    let err = verify_fixture_host_binaries(&config, &manifest_path).unwrap_err();

    match err {
        PreflightError::BinaryHashMismatch { name, path, .. } => {
            assert_eq!(name, "m80_net_helper");
            assert_eq!(path, system_binary);
        }
        other => panic!("expected helper hash mismatch, got {other:?}"),
    }
}

#[cfg(target_os = "linux")]
#[test]
fn host_binary_manifest_firecracker_version_mismatch_fails_closed() {
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
        system_binary,
    );
    let mut manifest = HostBinariesManifest::read(&manifest_path).unwrap();
    let firecracker = manifest
        .binaries
        .iter_mut()
        .find(|entry| entry.name == HostBinaryName::Firecracker)
        .unwrap();
    firecracker.version = "v1.14.4".to_owned();
    manifest.write(&manifest_path).unwrap();
    let config = BinaryDiscoveryConfig {
        firecracker_bin: system_binary.to_path_buf(),
        firecracker_seccomp_filter: system_binary.to_path_buf(),
        jailer_bin: system_binary.to_path_buf(),
        jailer_harden_bin: system_binary.to_path_buf(),
        net_helper_bin: system_binary.to_path_buf(),
        expected_firecracker_version: None,
    };

    let err = verify_fixture_host_binaries(&config, &manifest_path).unwrap_err();

    match err {
        PreflightError::HostBinaryVersionMismatch {
            name,
            path,
            expected,
            actual,
        } => {
            assert_eq!(name, "firecracker");
            assert_eq!(path, system_binary);
            assert_eq!(expected, "v1.14.4");
            assert_eq!(actual, "v1.15.1");
        }
        other => panic!("expected firecracker version mismatch, got {other:?}"),
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
        &config.firecracker_seccomp_filter,
        &config.jailer_bin,
        &config.jailer_harden_bin,
        &config.net_helper_bin,
        &config.firecracker_bin,
    );

    let err = verify_fixture_host_binaries(&config, &manifest_path).unwrap_err();

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
        &config.firecracker_seccomp_filter,
        &config.jailer_bin,
        &config.jailer_harden_bin,
        &config.net_helper_bin,
        &config.firecracker_bin,
    );

    let err = verify_fixture_host_binaries(&config, &manifest_path).unwrap_err();

    match err {
        PreflightError::HostBinaryPermission { name, path, .. } => {
            assert_eq!(name, "firecracker");
            assert_eq!(path, config.firecracker_bin);
        }
        other => panic!("expected host binary permission rejection, got {other:?}"),
    }
}

#[cfg(target_os = "linux")]
#[test]
fn host_binary_manifest_rejects_non_executable_binary() {
    let dir = tempfile::tempdir().unwrap();
    let system_binary = system_root_owned_executable();
    let non_executable = system_root_owned_non_executable();
    let manifest_path = dir.path().join("host-binaries.manifest.json");
    write_host_binary_manifest(
        &manifest_path,
        system_binary,
        system_binary,
        system_binary,
        system_binary,
        system_binary,
        system_binary,
    );
    let mut manifest = HostBinariesManifest::read(&manifest_path).unwrap();
    let m80 = manifest
        .binaries
        .iter_mut()
        .find(|entry| entry.name == HostBinaryName::M80)
        .unwrap();
    m80.path = non_executable.to_path_buf();
    m80.sha256 = sha256_file(non_executable);
    m80.version = "not probed before permission rejection".to_owned();
    manifest.write(&manifest_path).unwrap();
    let config = BinaryDiscoveryConfig {
        firecracker_bin: system_binary.to_path_buf(),
        firecracker_seccomp_filter: system_binary.to_path_buf(),
        jailer_bin: system_binary.to_path_buf(),
        jailer_harden_bin: system_binary.to_path_buf(),
        net_helper_bin: system_binary.to_path_buf(),
        expected_firecracker_version: None,
    };

    let err = verify_fixture_host_binaries(&config, &manifest_path).unwrap_err();

    match err {
        PreflightError::HostBinaryPermission { name, path, reason } => {
            assert_eq!(name, "m80");
            assert_eq!(path, non_executable);
            assert_eq!(reason, "not executable");
        }
        other => panic!("expected non-executable host binary rejection, got {other:?}"),
    }
}

#[cfg(target_os = "linux")]
#[test]
fn host_binary_manifest_symlink_fails_no_follow_open() {
    let dir = tempfile::tempdir().unwrap();
    let system_binary = system_root_owned_executable();
    let m80_link = dir.path().join("m80-link");
    std::os::unix::fs::symlink(system_binary, &m80_link).unwrap();
    let manifest_path = dir.path().join("host-binaries.manifest.json");
    write_host_binary_manifest(
        &manifest_path,
        system_binary,
        system_binary,
        system_binary,
        system_binary,
        system_binary,
        system_binary,
    );
    let mut manifest = HostBinariesManifest::read(&manifest_path).unwrap();
    let m80 = manifest
        .binaries
        .iter_mut()
        .find(|entry| entry.name == HostBinaryName::M80)
        .unwrap();
    m80.path = m80_link.clone();
    m80.sha256 = sha256_file(&m80_link);
    m80.version = binary_version_stdout(&m80_link);
    manifest.write(&manifest_path).unwrap();
    let config = BinaryDiscoveryConfig {
        firecracker_bin: system_binary.to_path_buf(),
        firecracker_seccomp_filter: system_binary.to_path_buf(),
        jailer_bin: system_binary.to_path_buf(),
        jailer_harden_bin: system_binary.to_path_buf(),
        net_helper_bin: system_binary.to_path_buf(),
        expected_firecracker_version: None,
    };

    let err = verify_fixture_host_binaries(&config, &manifest_path).unwrap_err();

    match err {
        PreflightError::PathIo { path, source } => {
            assert_eq!(path, m80_link);
            assert_eq!(source.raw_os_error(), Some(nix::libc::ELOOP));
        }
        other => panic!("expected O_NOFOLLOW host binary symlink failure, got {other:?}"),
    }
}

#[cfg(target_os = "linux")]
#[test]
fn host_binary_manifest_requires_seccomp_launch_material() {
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
        system_binary,
    );
    let mut manifest = HostBinariesManifest::read(&manifest_path).unwrap();
    manifest.launch_material.clear();
    manifest.write(&manifest_path).unwrap();
    let config = BinaryDiscoveryConfig {
        firecracker_bin: system_binary.to_path_buf(),
        firecracker_seccomp_filter: system_binary.to_path_buf(),
        jailer_bin: system_binary.to_path_buf(),
        jailer_harden_bin: system_binary.to_path_buf(),
        net_helper_bin: system_binary.to_path_buf(),
        expected_firecracker_version: None,
    };

    let err = verify_fixture_host_binaries(&config, &manifest_path).unwrap_err();

    match err {
        PreflightError::HostLaunchMaterialMissing { name } => {
            assert_eq!(name, "firecracker_seccomp_filter");
        }
        other => panic!("expected launch material missing, got {other:?}"),
    }
}

#[cfg(target_os = "linux")]
#[test]
fn host_binary_manifest_rejects_duplicate_seccomp_launch_material() {
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
        system_binary,
    );
    let mut manifest = HostBinariesManifest::read(&manifest_path).unwrap();
    manifest
        .launch_material
        .push(manifest.launch_material[0].clone());
    manifest.write(&manifest_path).unwrap();
    let config = BinaryDiscoveryConfig {
        firecracker_bin: system_binary.to_path_buf(),
        firecracker_seccomp_filter: system_binary.to_path_buf(),
        jailer_bin: system_binary.to_path_buf(),
        jailer_harden_bin: system_binary.to_path_buf(),
        net_helper_bin: system_binary.to_path_buf(),
        expected_firecracker_version: None,
    };

    let err = verify_fixture_host_binaries(&config, &manifest_path).unwrap_err();

    match err {
        PreflightError::HostLaunchMaterialDuplicate { name } => {
            assert_eq!(name, "firecracker_seccomp_filter");
        }
        other => panic!("expected launch material duplicate, got {other:?}"),
    }
}

#[cfg(target_os = "linux")]
#[test]
fn host_launch_material_hash_mismatch_fails_closed() {
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
        system_binary,
    );
    let mut manifest = HostBinariesManifest::read(&manifest_path).unwrap();
    manifest.launch_material[0].sha256 = "0".repeat(64);
    manifest.write(&manifest_path).unwrap();
    let config = BinaryDiscoveryConfig {
        firecracker_bin: system_binary.to_path_buf(),
        firecracker_seccomp_filter: system_binary.to_path_buf(),
        jailer_bin: system_binary.to_path_buf(),
        jailer_harden_bin: system_binary.to_path_buf(),
        net_helper_bin: system_binary.to_path_buf(),
        expected_firecracker_version: None,
    };

    let err = verify_fixture_host_binaries(&config, &manifest_path).unwrap_err();

    match err {
        PreflightError::HostLaunchMaterialHashMismatch { name, path, .. } => {
            assert_eq!(name, "firecracker_seccomp_filter");
            assert_eq!(path, system_binary);
        }
        other => panic!("expected launch material hash mismatch, got {other:?}"),
    }
}

#[cfg(target_os = "linux")]
#[test]
fn host_launch_material_version_mismatch_fails_closed() {
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
        system_binary,
    );
    let mut manifest = HostBinariesManifest::read(&manifest_path).unwrap();
    manifest.launch_material[0].version = "v1.14.4".to_owned();
    manifest.write(&manifest_path).unwrap();
    let config = BinaryDiscoveryConfig {
        firecracker_bin: system_binary.to_path_buf(),
        firecracker_seccomp_filter: system_binary.to_path_buf(),
        jailer_bin: system_binary.to_path_buf(),
        jailer_harden_bin: system_binary.to_path_buf(),
        net_helper_bin: system_binary.to_path_buf(),
        expected_firecracker_version: None,
    };

    let err = verify_fixture_host_binaries(&config, &manifest_path).unwrap_err();

    match err {
        PreflightError::HostLaunchMaterialVersionMismatch {
            name,
            path,
            expected,
            actual,
        } => {
            assert_eq!(name, "firecracker_seccomp_filter");
            assert_eq!(path, system_binary);
            assert_eq!(expected, "v1.14.4");
            assert_eq!(actual, "v1.15.1");
        }
        other => panic!("expected launch material version mismatch, got {other:?}"),
    }
}

#[cfg(target_os = "linux")]
#[test]
fn host_launch_material_rejects_path_mismatch() {
    let dir = tempfile::tempdir().unwrap();
    let system_binary = system_root_owned_executable();
    let other_seccomp = dir.path().join("other-seccomp-filter.bin");
    write_seccomp_filter(&other_seccomp);
    let manifest_path = dir.path().join("host-binaries.manifest.json");
    write_host_binary_manifest(
        &manifest_path,
        system_binary,
        &other_seccomp,
        system_binary,
        system_binary,
        system_binary,
        system_binary,
    );
    let config = BinaryDiscoveryConfig {
        firecracker_bin: system_binary.to_path_buf(),
        firecracker_seccomp_filter: system_binary.to_path_buf(),
        jailer_bin: system_binary.to_path_buf(),
        jailer_harden_bin: system_binary.to_path_buf(),
        net_helper_bin: system_binary.to_path_buf(),
        expected_firecracker_version: None,
    };

    let err = verify_fixture_host_binaries(&config, &manifest_path).unwrap_err();

    match err {
        PreflightError::HostLaunchMaterialPathMismatch {
            name,
            expected,
            actual,
        } => {
            assert_eq!(name, "firecracker_seccomp_filter");
            assert_eq!(expected, system_binary);
            assert_eq!(actual, other_seccomp);
        }
        other => panic!("expected launch material path mismatch, got {other:?}"),
    }
}

#[cfg(target_os = "linux")]
#[test]
fn host_launch_material_rejects_empty_file() {
    let dir = tempfile::tempdir().unwrap();
    let system_binary = system_root_owned_executable();
    let seccomp = dir.path().join("empty-seccomp-filter.bin");
    fs::write(&seccomp, b"").unwrap();
    let manifest_path = dir.path().join("host-binaries.manifest.json");
    write_host_binary_manifest(
        &manifest_path,
        system_binary,
        &seccomp,
        system_binary,
        system_binary,
        system_binary,
        system_binary,
    );
    let config = BinaryDiscoveryConfig {
        firecracker_bin: system_binary.to_path_buf(),
        firecracker_seccomp_filter: seccomp.clone(),
        jailer_bin: system_binary.to_path_buf(),
        jailer_harden_bin: system_binary.to_path_buf(),
        net_helper_bin: system_binary.to_path_buf(),
        expected_firecracker_version: None,
    };

    let err = verify_fixture_host_binaries(&config, &manifest_path).unwrap_err();

    match err {
        PreflightError::HostLaunchMaterialPermission { name, path, reason } => {
            assert_eq!(name, "firecracker_seccomp_filter");
            assert_eq!(path, seccomp);
            assert_eq!(reason, "empty file");
        }
        other => panic!("expected launch material empty-file rejection, got {other:?}"),
    }
}

#[cfg(target_os = "linux")]
#[test]
fn host_launch_material_rejects_unsafe_permissions() {
    let dir = tempfile::tempdir().unwrap();
    let system_binary = system_root_owned_executable();
    let seccomp = dir.path().join("user-owned-seccomp-filter.bin");
    write_seccomp_filter(&seccomp);
    let manifest_path = dir.path().join("host-binaries.manifest.json");
    write_host_binary_manifest(
        &manifest_path,
        system_binary,
        &seccomp,
        system_binary,
        system_binary,
        system_binary,
        system_binary,
    );
    let config = BinaryDiscoveryConfig {
        firecracker_bin: system_binary.to_path_buf(),
        firecracker_seccomp_filter: seccomp.clone(),
        jailer_bin: system_binary.to_path_buf(),
        jailer_harden_bin: system_binary.to_path_buf(),
        net_helper_bin: system_binary.to_path_buf(),
        expected_firecracker_version: None,
    };

    let err = verify_fixture_host_binaries(&config, &manifest_path).unwrap_err();

    match err {
        PreflightError::HostLaunchMaterialPermission { name, path, reason } => {
            assert_eq!(name, "firecracker_seccomp_filter");
            assert_eq!(path, seccomp);
            assert_eq!(reason, "owner is not root:root");
        }
        other => panic!("expected launch material permission rejection, got {other:?}"),
    }
}

#[cfg(target_os = "linux")]
#[test]
fn host_launch_material_symlink_fails_no_follow_open() {
    let dir = tempfile::tempdir().unwrap();
    let system_binary = system_root_owned_executable();
    let target = dir.path().join("seccomp-filter-target.bin");
    let seccomp = dir.path().join("seccomp-filter-link.bin");
    write_seccomp_filter(&target);
    std::os::unix::fs::symlink(&target, &seccomp).unwrap();
    let manifest_path = dir.path().join("host-binaries.manifest.json");
    write_host_binary_manifest(
        &manifest_path,
        system_binary,
        &seccomp,
        system_binary,
        system_binary,
        system_binary,
        system_binary,
    );
    let config = BinaryDiscoveryConfig {
        firecracker_bin: system_binary.to_path_buf(),
        firecracker_seccomp_filter: seccomp.clone(),
        jailer_bin: system_binary.to_path_buf(),
        jailer_harden_bin: system_binary.to_path_buf(),
        net_helper_bin: system_binary.to_path_buf(),
        expected_firecracker_version: None,
    };

    let err = verify_fixture_host_binaries(&config, &manifest_path).unwrap_err();

    match err {
        PreflightError::PathIo { path, source } => {
            assert_eq!(path, seccomp);
            assert_eq!(source.raw_os_error(), Some(nix::libc::ELOOP));
        }
        other => panic!("expected O_NOFOLLOW symlink failure, got {other:?}"),
    }
}

#[cfg(target_os = "linux")]
#[test]
fn host_launch_material_can_be_non_executable_and_uses_firecracker_train_version() {
    let dir = tempfile::tempdir().unwrap();
    let system_binary = system_root_owned_executable();
    let non_executable = system_root_owned_non_executable();
    let manifest_path = dir.path().join("host-binaries.manifest.json");
    write_host_binary_manifest(
        &manifest_path,
        system_binary,
        non_executable,
        system_binary,
        system_binary,
        system_binary,
        system_binary,
    );
    let config = BinaryDiscoveryConfig {
        firecracker_bin: system_binary.to_path_buf(),
        firecracker_seccomp_filter: non_executable.to_path_buf(),
        jailer_bin: system_binary.to_path_buf(),
        jailer_harden_bin: system_binary.to_path_buf(),
        net_helper_bin: system_binary.to_path_buf(),
        expected_firecracker_version: None,
    };

    verify_fixture_host_binaries(&config, &manifest_path).unwrap();
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
