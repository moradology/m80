//! Hostless install-root fixture matrix for release prerequisite checks.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use m80_image_manifest::{
    HostBinariesManifest, HostBinaryName, HostLaunchMaterialName, HOST_BINARIES_SCHEMA_VERSION,
};
use m80_preflight::{
    verify_host_substrate_fixture, write_host_binaries_manifest, BinaryDiscoveryConfig,
    CgroupPreflightMode, HostBinariesManifestConfig, HostFeaturePreflightConfig,
    HostPrerequisiteStatus, HostSubstrateDiscovery, HostSubstrateFixture, HostSubstrateProofKind,
    PreflightError, HOST_PREREQUISITE_RESULT_SCHEMA_VERSION,
};
use sha2::{Digest, Sha256};

const DEFAULT_FIRECRACKER_VERSION: &str = "v1.15.1";

struct InstallRootFixture {
    _temp: tempfile::TempDir,
    install_root: PathBuf,
    artifact_dir: PathBuf,
    run_root: PathBuf,
    active_profile: PathBuf,
    firecracker_bin: PathBuf,
    firecracker_seccomp_filter: PathBuf,
    jailer_bin: PathBuf,
    jailer_harden_bin: PathBuf,
    net_helper_bin: PathBuf,
    m80_bin: PathBuf,
    expected_firecracker_version: String,
}

struct HostPrerequisiteFixtureProof {
    substrate: HostSubstrateDiscovery,
    host_binaries_manifest: HostBinariesManifest,
    artifact_dir: PathBuf,
    run_root: PathBuf,
    active_profile: PathBuf,
}

impl InstallRootFixture {
    fn new() -> Self {
        Self::with_firecracker_versions(DEFAULT_FIRECRACKER_VERSION, DEFAULT_FIRECRACKER_VERSION)
    }

    fn with_installed_firecracker_version(installed_firecracker_version: &str) -> Self {
        Self::with_firecracker_versions(installed_firecracker_version, DEFAULT_FIRECRACKER_VERSION)
    }

    fn with_firecracker_versions(
        installed_firecracker_version: &str,
        expected_firecracker_version: &str,
    ) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let install_root = temp.path().join("install-root");
        let firecracker_dir = install_root.join("opt/firecracker/bin");
        let m80_bin_dir = install_root.join("opt/m80/bin");
        let artifact_dir = install_root.join("opt/m80/artifacts");
        let run_root = install_root.join("var/run/m80");
        let profile_dir = install_root.join("etc/m80/profiles");
        fs::create_dir_all(&firecracker_dir).unwrap();
        fs::create_dir_all(&m80_bin_dir).unwrap();
        fs::create_dir_all(&artifact_dir).unwrap();
        fs::create_dir_all(&run_root).unwrap();
        fs::create_dir_all(&profile_dir).unwrap();

        let firecracker_bin = firecracker_dir.join("firecracker");
        let firecracker_seccomp_filter = firecracker_dir.join("firecracker-seccomp-filter.bin");
        let jailer_bin = firecracker_dir.join("jailer");
        let jailer_harden_bin = m80_bin_dir.join("m80-jailer-harden");
        let net_helper_bin = m80_bin_dir.join("m80-net-helper");
        let m80_bin = m80_bin_dir.join("m80");
        let active_profile = profile_dir.join("default.toml");

        write_executable(
            &firecracker_bin,
            &format!("#!/bin/sh\nprintf 'Firecracker {installed_firecracker_version}\\n'\n"),
        );
        write_executable(
            &jailer_bin,
            &format!("#!/bin/sh\nprintf 'Jailer {installed_firecracker_version}\\n'\n"),
        );
        write_executable(
            &jailer_harden_bin,
            "#!/bin/sh\nif [ \"$1\" = --version ]; then printf 'm80-jailer-harden 0.0.0\\n'; fi\n",
        );
        write_executable(
            &net_helper_bin,
            "#!/bin/sh\nif [ \"$1\" = --version ]; then printf 'm80-net-helper 0.0.0\\n'; fi\n",
        );
        write_executable(
            &m80_bin,
            "#!/bin/sh\nif [ \"$1\" = --version ]; then printf 'm80 0.0.0\\n'; fi\n",
        );
        fs::write(&firecracker_seccomp_filter, b"{\"seccomp_level\":2}\n").unwrap();
        fs::write(
            &active_profile,
            format!(
                "kernel_image = \"{}\"\nrootfs_image = \"{}\"\nkernel_kind = \"stock\"\n",
                artifact_dir.join("vmlinux").display(),
                artifact_dir.join("output.ext4").display()
            ),
        )
        .unwrap();

        Self {
            _temp: temp,
            install_root,
            artifact_dir,
            run_root,
            active_profile,
            firecracker_bin,
            firecracker_seccomp_filter,
            jailer_bin,
            jailer_harden_bin,
            net_helper_bin,
            m80_bin,
            expected_firecracker_version: expected_firecracker_version.to_owned(),
        }
    }

    fn root(&self) -> &Path {
        &self.install_root
    }

    fn manifest_path(&self) -> PathBuf {
        self.artifact_dir.join("host-binaries.manifest.json")
    }

    fn binary_config(&self) -> BinaryDiscoveryConfig {
        BinaryDiscoveryConfig {
            firecracker_bin: self.firecracker_bin.clone(),
            firecracker_seccomp_filter: self.firecracker_seccomp_filter.clone(),
            jailer_bin: self.jailer_bin.clone(),
            jailer_harden_bin: self.jailer_harden_bin.clone(),
            net_helper_bin: self.net_helper_bin.clone(),
            expected_firecracker_version: Some(self.expected_firecracker_version.clone()),
        }
    }

    fn manifest_config(&self) -> HostBinariesManifestConfig {
        let binary = self.binary_config();
        HostBinariesManifestConfig {
            firecracker_bin: binary.firecracker_bin,
            firecracker_seccomp_filter: binary.firecracker_seccomp_filter,
            jailer_bin: binary.jailer_bin,
            jailer_harden_bin: binary.jailer_harden_bin,
            net_helper_bin: binary.net_helper_bin,
            m80_bin: self.m80_bin.clone(),
            expected_firecracker_version: binary.expected_firecracker_version,
        }
    }

    fn host_feature_config(&self) -> HostFeaturePreflightConfig {
        HostFeaturePreflightConfig {
            cgroup_mode: CgroupPreflightMode::UnifiedV2,
            jail_uid: 3000,
            jail_gid: 3000,
            expected_concurrent_vms: 8,
        }
    }

    fn write_manifest(&self) -> Result<HostBinariesManifest, PreflightError> {
        let manifest_path = self.manifest_path();
        write_host_binaries_manifest(&self.manifest_config(), &manifest_path)?;
        HostBinariesManifest::read(&manifest_path).map_err(PreflightError::HostBinaryManifest)
    }

    fn prove_host_prerequisites(&self) -> Result<HostPrerequisiteFixtureProof, PreflightError> {
        let substrate = verify_host_substrate_fixture(
            self.host_feature_config(),
            &HostSubstrateFixture::supported_root(),
        )?;
        let host_binaries_manifest = self.write_manifest()?;
        Ok(HostPrerequisiteFixtureProof {
            substrate,
            host_binaries_manifest,
            artifact_dir: self.artifact_dir.clone(),
            run_root: self.run_root.clone(),
            active_profile: self.active_profile.clone(),
        })
    }

    fn assert_path_in_install_root(&self, path: &Path) {
        assert!(
            path.starts_with(self.root()),
            "{} should stay under {}",
            path.display(),
            self.root().display()
        );
    }
}

#[test]
fn install_root_fixture_success_emits_substrate_and_manifest_shapes() {
    let fixture = InstallRootFixture::new();

    let proof = fixture.prove_host_prerequisites().unwrap();

    assert_eq!(
        proof.substrate.proof_kind,
        HostSubstrateProofKind::HostlessFixture
    );
    assert!(proof
        .substrate
        .report
        .iter()
        .any(|row| row.label == "Host substrate proof"
            && row.detail.contains("hostless fixture only")));
    assert_eq!(
        proof.substrate.host_prerequisites.schema_version,
        HOST_PREREQUISITE_RESULT_SCHEMA_VERSION
    );
    assert!(proof
        .substrate
        .host_prerequisites
        .checks
        .iter()
        .any(|check| check.check_name == "Host substrate proof"
            && check.status == HostPrerequisiteStatus::Pass));
    assert_eq!(
        proof.host_binaries_manifest.schema_version(),
        HOST_BINARIES_SCHEMA_VERSION
    );
    assert_eq!(proof.host_binaries_manifest.binaries.len(), 5);
    assert_eq!(proof.host_binaries_manifest.launch_material.len(), 1);
    for name in [
        HostBinaryName::Firecracker,
        HostBinaryName::Jailer,
        HostBinaryName::M80,
        HostBinaryName::M80JailerHarden,
        HostBinaryName::M80NetHelper,
    ] {
        assert!(
            proof
                .host_binaries_manifest
                .binaries
                .iter()
                .any(|entry| entry.name == name),
            "fixture manifest missing {name:?}"
        );
    }
    assert!(
        proof
            .host_binaries_manifest
            .launch_material
            .iter()
            .any(|entry| entry.name == HostLaunchMaterialName::FirecrackerSeccompFilter),
        "fixture manifest missing seccomp launch material"
    );
    for entry in &proof.host_binaries_manifest.binaries {
        fixture.assert_path_in_install_root(&entry.path);
    }
    for entry in &proof.host_binaries_manifest.launch_material {
        fixture.assert_path_in_install_root(&entry.path);
    }
    for path in [
        proof.artifact_dir.as_path(),
        proof.run_root.as_path(),
        proof.active_profile.as_path(),
    ] {
        fixture.assert_path_in_install_root(path);
        assert!(path.exists(), "{} should exist", path.display());
    }
}

#[test]
fn install_root_fixture_rejects_wrong_firecracker_train() {
    let fixture = InstallRootFixture::with_installed_firecracker_version("v1.14.4");

    let err = fixture.write_manifest().unwrap_err();

    match err {
        PreflightError::FirecrackerVersionMismatch {
            expected, actual, ..
        } => {
            assert_eq!(expected, "v1.15.1");
            assert_eq!(actual, "v1.14.4");
        }
        other => panic!("expected Firecracker version mismatch, got {other:?}"),
    }
}

#[test]
fn install_root_fixture_records_stale_hash_observable_after_manifest_generation() {
    let fixture = InstallRootFixture::new();
    let manifest = fixture.write_manifest().unwrap();
    let helper = manifest
        .binaries
        .iter()
        .find(|entry| entry.name == HostBinaryName::M80NetHelper)
        .expect("net helper entry");

    let recorded_hash = helper.sha256.clone();
    write_executable(
        &fixture.net_helper_bin,
        "#!/bin/sh\nif [ \"$1\" = --version ]; then printf 'm80-net-helper 0.0.1\\n'; fi\n",
    );
    let actual_hash = sha256_file(&fixture.net_helper_bin);

    assert_ne!(
        recorded_hash, actual_hash,
        "fixture should surface stale installed bytes as a manifest hash drift"
    );
}

#[test]
fn install_root_fixture_rejects_missing_jailer() {
    let fixture = InstallRootFixture::new();
    fs::remove_file(&fixture.jailer_bin).unwrap();

    let err = fixture.write_manifest().unwrap_err();

    assert!(matches!(err, PreflightError::JailerBinaryNotFound));
}

#[test]
fn install_root_fixture_rejects_missing_seccomp_filter() {
    let fixture = InstallRootFixture::new();
    fs::remove_file(&fixture.firecracker_seccomp_filter).unwrap();

    let err = fixture.write_manifest().unwrap_err();

    match err {
        PreflightError::FirecrackerSeccompFilterNotFound { path } => {
            assert_eq!(path, fixture.firecracker_seccomp_filter);
        }
        other => panic!("expected missing seccomp filter, got {other:?}"),
    }
}

#[test]
fn install_root_fixture_rejects_helper_that_cannot_run_version_probe() {
    let fixture = InstallRootFixture::new();
    let mut permissions = fs::metadata(&fixture.net_helper_bin).unwrap().permissions();
    permissions.set_mode(0o644);
    fs::set_permissions(&fixture.net_helper_bin, permissions).unwrap();

    let err = fixture.write_manifest().unwrap_err();

    match err {
        PreflightError::PathIo { path, source } => {
            assert_eq!(path, fixture.net_helper_bin);
            assert_eq!(source.kind(), std::io::ErrorKind::PermissionDenied);
        }
        other => panic!("expected non-executable helper path I/O error, got {other:?}"),
    }
}

#[test]
fn install_root_override_keeps_all_final_paths_under_temp_root() {
    let fixture = InstallRootFixture::new();
    let binary = fixture.binary_config();
    let manifest = fixture.manifest_config();

    for path in [
        binary.firecracker_bin.as_path(),
        binary.firecracker_seccomp_filter.as_path(),
        binary.jailer_bin.as_path(),
        binary.jailer_harden_bin.as_path(),
        binary.net_helper_bin.as_path(),
        manifest.m80_bin.as_path(),
        fixture.artifact_dir.as_path(),
        fixture.run_root.as_path(),
        fixture.active_profile.as_path(),
    ] {
        fixture.assert_path_in_install_root(path);
    }
}

fn write_executable(path: &Path, body: &str) {
    fs::write(path, body).unwrap();
    let mut permissions = fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).unwrap();
}

fn sha256_file(path: &Path) -> String {
    let bytes = fs::read(path).unwrap();
    hex::encode(Sha256::digest(bytes))
}
