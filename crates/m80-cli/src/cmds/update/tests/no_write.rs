use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt as _};
use std::path::{Path, PathBuf};

use m80_image_manifest::{
    HostBinariesManifest, HostBinaryEntry, HostBinaryName, HostLaunchMaterialEntry,
    HostLaunchMaterialName, InstallProvenance, InstallProvenanceArtifact, InstallProvenanceRewrite,
    InstallProvenanceTransform,
};
use serde::Serialize;
use sha2::{Digest, Sha256};

use super::super::cmd_update;
use crate::args::UpdateArgs;

#[test]
fn update_check_does_not_write_files_on_healthy_latest_status_path() {
    let fixture = HealthyInstallFixture::new();
    fixture.write_installed_release("v1.2.3");
    let latest_status = fixture.temp.path().join("latest-status.json");
    fs::write(
        &latest_status,
        super::status_artifact("v1.2.4", Some("v1.2.0"), &[]),
    )
    .expect("write latest status fixture");
    let before = snapshot_tree(fixture.temp.path());

    let status = cmd_update(
        UpdateArgs {
            check: true,
            install_root: fixture.install_root(),
            profile: None,
            latest_status: Some(latest_status),
            latest_status_url: None,
            config_path: Some(fixture.config_path()),
            profile_dir: Some(fixture.profile_dir()),
        },
        false,
    )
    .expect("update check should run");

    assert_eq!(status, 0);
    assert_eq!(snapshot_tree(fixture.temp.path()), before);
}

struct HealthyInstallFixture {
    temp: tempfile::TempDir,
    _env_restore: m80_test_helpers::env::EnvRestore,
    _env_lock: std::sync::MutexGuard<'static, ()>,
}

impl HealthyInstallFixture {
    fn new() -> Self {
        let env_lock = m80_test_helpers::env::env_lock().lock().unwrap();
        let env_restore = m80_test_helpers::env::EnvRestore::capture(&[
            "M80_DEFAULT_PROFILE",
            "M80_RUN_ROOT",
            "M80_MAX_CONCURRENT_VMS",
            "M80_JAIL_UID",
            "M80_JAIL_GID",
            "M80_CGROUP_MODE",
        ]);
        for key in [
            "M80_DEFAULT_PROFILE",
            "M80_RUN_ROOT",
            "M80_MAX_CONCURRENT_VMS",
            "M80_JAIL_UID",
            "M80_JAIL_GID",
            "M80_CGROUP_MODE",
        ] {
            std::env::remove_var(key);
        }
        Self {
            temp: tempfile::tempdir().expect("create fixture tempdir"),
            _env_restore: env_restore,
            _env_lock: env_lock,
        }
    }

    fn install_root(&self) -> PathBuf {
        self.temp.path().join("install")
    }

    fn profile_dir(&self) -> PathBuf {
        self.temp.path().join("profiles")
    }

    fn config_path(&self) -> PathBuf {
        self.temp.path().join("config.toml")
    }

    fn write_installed_release(&self, tag: &str) {
        self.write_installed_profile(tag);
        self.write_install_metadata(tag);
        fs::write(self.config_path(), "default_profile = 'default'\n")
            .expect("write fixture config");
        let version_dir = self.install_root().join("versions").join(tag);
        symlink(version_dir, self.install_root().join("active"))
            .expect("point active at fixture release");
    }

    fn write_installed_profile(&self, tag: &str) {
        let version_dir = self.install_root().join("versions").join(tag);
        let artifacts = version_dir.join("artifacts");
        let bin = version_dir.join("bin");
        fs::create_dir_all(self.profile_dir()).expect("create profile dir");
        fs::create_dir_all(&artifacts).expect("create artifacts dir");
        fs::create_dir_all(&bin).expect("create bin dir");
        fs::write(
            self.profile_dir().join("default.toml"),
            format!(
                "artifact_dir = '{}'\n\
                 kernel_image = '{}'\n\
                 rootfs_image = '{}'\n\
                 kernel_kind = 'stripped'\n\
                 guestd = '{}'\n\
                 guest_manifest = '{}'\n\
                 build_receipt = '{}'\n\
                 install_provenance = '{}'\n\
                 host_binaries_manifest = '{}'\n\
                 firecracker_bin = '/opt/firecracker/bin/firecracker'\n\
                 firecracker_seccomp_filter = '/opt/firecracker/bin/firecracker-seccomp-filter.bin'\n\
                 jailer_bin = '/opt/firecracker/bin/jailer'\n\
                 jailer_harden_bin = '{}'\n\
                 net_helper_bin = '{}'\n\
                 release_tag = '{}'\n\
                 m80_version = '{}'\n",
                artifacts.display(),
                artifacts.join("vmlinux").display(),
                artifacts.join("output.ext4").display(),
                artifacts.join("m80-guestd").display(),
                artifacts.join("output.ext4.manifest.json").display(),
                artifacts.join("output.ext4.build-receipt.json").display(),
                artifacts.join("install-provenance.json").display(),
                artifacts.join("host-binaries.manifest.json").display(),
                bin.join("m80-jailer-harden").display(),
                bin.join("m80-net-helper").display(),
                tag,
                tag
            ),
        )
        .expect("write fixture profile");
    }

    fn write_install_metadata(&self, tag: &str) {
        let version_dir = self.install_root().join("versions").join(tag);
        let artifacts = version_dir.join("artifacts");
        let bin = version_dir.join("bin");
        fs::create_dir_all(&artifacts).expect("create artifacts dir");
        fs::create_dir_all(&bin).expect("create bin dir");
        let bundle_files = [
            "bin/m80",
            "bin/m80-jailer-harden",
            "bin/m80-net-helper",
            "artifacts/vmlinux",
            "artifacts/output.ext4",
            "artifacts/output.ext4.manifest.json",
            "artifacts/output.ext4.build-receipt.json",
            "artifacts/m80-guestd",
            "install.sh",
        ];
        let files = bundle_files
            .iter()
            .map(|relative| {
                let path = version_dir.join(relative);
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent).expect("create bundle file parent");
                }
                fs::write(&path, format!("{relative} for {tag}\n")).expect("write bundle file");
                let bytes = fs::read(&path).expect("read bundle file");
                serde_json::json!({
                    "path": relative,
                    "sha256": sha256_bytes(&bytes),
                    "size_bytes": bytes.len(),
                })
            })
            .collect::<Vec<_>>();
        fs::write(
            version_dir.join("bundle.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "schema_version": 1,
                "release_tag": tag,
                "m80_version": tag,
                "package_version": tag,
                "target": "linux-x86_64",
                "os": "linux",
                "arch": "x86_64",
                "image_kind": "minimal",
                "m80_protocol_version": 1,
                "guestd_package_version": tag,
                "guest_protocol_version": 1,
                "manifest_schema_version": 1,
                "build_receipt_schema_version": 1,
                "build_receipt_manifest_path": "artifacts/output.ext4.manifest.json",
                "install_provenance_schema_version": 1,
                "install_provenance_required": true,
                "expected_firecracker_version": "v1.15.1",
                "files": files,
            }))
            .expect("encode bundle metadata"),
        )
        .expect("write bundle metadata");

        let guest_manifest = artifacts.join("output.ext4.manifest.json");
        let build_receipt = artifacts.join("output.ext4.build-receipt.json");
        InstallProvenance::new(
            Some(tag.to_owned()),
            vec![
                install_transform(
                    InstallProvenanceArtifact::GuestManifest,
                    "artifacts/output.ext4.manifest.json",
                    &guest_manifest,
                ),
                install_transform(
                    InstallProvenanceArtifact::BuildReceipt,
                    "artifacts/output.ext4.build-receipt.json",
                    &build_receipt,
                ),
            ],
        )
        .write(&artifacts.join("install-provenance.json"))
        .expect("write install provenance");

        HostBinariesManifest::new(
            vec![host_binary(
                HostBinaryName::Firecracker,
                "/opt/firecracker/bin/firecracker",
            )],
            vec![host_material(
                HostLaunchMaterialName::FirecrackerSeccompFilter,
                "/opt/firecracker/bin/firecracker-seccomp-filter.bin",
            )],
        )
        .write(&artifacts.join("host-binaries.manifest.json"))
        .expect("write host-binaries manifest");
        write_proof_cache(&artifacts.join("release-proof-cache"), tag);
    }
}

fn snapshot_tree(root: &Path) -> Vec<String> {
    let mut entries = Vec::new();
    snapshot_tree_inner(root, root, &mut entries);
    entries
}

fn snapshot_tree_inner(root: &Path, dir: &Path, entries: &mut Vec<String>) {
    let mut children = fs::read_dir(dir)
        .expect("read snapshot dir")
        .map(|entry| entry.expect("read snapshot entry").path())
        .collect::<Vec<_>>();
    children.sort();
    for path in children {
        let metadata = fs::symlink_metadata(&path).expect("read snapshot metadata");
        let relative = path.strip_prefix(root).expect("strip snapshot root");
        if metadata.file_type().is_symlink() {
            entries.push(format!(
                "L\t{}\t{}",
                relative.display(),
                fs::read_link(&path).expect("read symlink target").display()
            ));
        } else if metadata.is_dir() {
            entries.push(format!("D\t{}", relative.display()));
            snapshot_tree_inner(root, &path, entries);
        } else {
            let bytes = fs::read(&path).expect("read snapshot file");
            entries.push(format!(
                "F\t{}\t{}\t{}",
                relative.display(),
                metadata.permissions().mode() & 0o777,
                sha256_bytes(&bytes)
            ));
        }
    }
}

fn install_transform(
    artifact: InstallProvenanceArtifact,
    source_path: &str,
    installed_path: &Path,
) -> InstallProvenanceTransform {
    let bytes = fs::read(installed_path).expect("read install provenance target");
    let digest = sha256_bytes(&bytes);
    InstallProvenanceTransform {
        artifact,
        source_sha256: digest.clone(),
        source_path: PathBuf::from(source_path),
        installed_sha256: digest,
        installed_path: installed_path.to_path_buf(),
        rewrite: InstallProvenanceRewrite::InstallPathRewrite,
    }
}

fn host_binary(name: HostBinaryName, path: &str) -> HostBinaryEntry {
    HostBinaryEntry {
        name,
        path: PathBuf::from(path),
        sha256: "a".repeat(64),
        version: "v1.15.1".to_owned(),
    }
}

fn host_material(name: HostLaunchMaterialName, path: &str) -> HostLaunchMaterialEntry {
    HostLaunchMaterialEntry {
        name,
        path: PathBuf::from(path),
        sha256: "b".repeat(64),
        version: "v1.15.1".to_owned(),
    }
}

fn write_proof_cache(cache_dir: &Path, tag: &str) {
    fs::create_dir_all(cache_dir).expect("create proof-cache dir");
    fs::set_permissions(cache_dir, fs::Permissions::from_mode(0o755))
        .expect("set proof-cache dir mode");
    let integrity = proof_file(cache_dir, "m80-release-integrity.json", b"integrity\n");
    let attestation = proof_file(
        cache_dir,
        "m80-release-integrity.attestation.jsonl",
        b"attestation\n",
    );
    let metadata = proof_file(cache_dir, "m80-release-attestation.json", b"metadata\n");
    let asset_index = proof_file(cache_dir, "m80-release-assets.json", b"asset-index\n");
    let public_sha256s = proof_file(cache_dir, "SHA256SUMS", b"sha256s\n");
    let sidecar = proof_file(cache_dir, "m80-linux-x86_64.tar.gz.sha256", b"abc bundle\n");
    let trust = proof_file(cache_dir, "m80-release-trust-policy.json", b"trust\n");
    let payload = TestProofPayload {
        release_tag: tag.to_owned(),
        repository: "moradology/m80".to_owned(),
        target: "linux-x86_64".to_owned(),
        integrity_predicate: integrity,
        attestation_bundle: attestation,
        attestation_metadata: TestAttestationMetadataRef {
            file: metadata,
            signer_identity:
                "https://github.com/moradology/m80/.github/workflows/release.yml@refs/tags/v1.2.3"
                    .to_owned(),
            issuer: "https://token.actions.githubusercontent.com".to_owned(),
            keyset_id: "keyset".to_owned(),
            predicate_sha256: "c".repeat(64),
        },
        asset_index,
        public_sha256s,
        checksum_sidecars: vec![TestChecksumSidecarRef {
            path: sidecar.path,
            sha256: sidecar.sha256,
            subject: "bundle".to_owned(),
        }],
        trust_policy: TestTrustPolicyRef {
            path: trust.path,
            identity: "repository=moradology/m80".to_owned(),
            sha256: trust.sha256,
        },
        verifier_versions: TestVerifierVersions {
            m80_version: tag.to_owned(),
            gh_version: "gh version 2.0.0".to_owned(),
            release_integrity_schema_version: 1,
            asset_index_schema_version: 1,
        },
    };
    let manifest = TestProofManifest {
        schema_version: 1,
        manifest_digest: sha256_json(&payload),
        payload,
    };
    fs::write(
        cache_dir.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest).expect("encode proof-cache manifest"),
    )
    .expect("write proof-cache manifest");
    fs::set_permissions(
        cache_dir.join("manifest.json"),
        fs::Permissions::from_mode(0o644),
    )
    .expect("set proof-cache manifest mode");
}

fn proof_file(cache_dir: &Path, name: &str, bytes: &[u8]) -> TestProofFile {
    let path = cache_dir.join(name);
    fs::write(&path, bytes).expect("write proof-cache file");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644))
        .expect("set proof-cache file mode");
    TestProofFile {
        path: name.to_owned(),
        sha256: sha256_bytes(bytes),
        size_bytes: bytes.len() as u64,
    }
}

fn sha256_json(value: &impl Serialize) -> String {
    sha256_bytes(&serde_json::to_vec(value).expect("encode proof-cache digest payload"))
}

fn sha256_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[derive(Serialize)]
struct TestProofManifest {
    schema_version: u32,
    manifest_digest: String,
    payload: TestProofPayload,
}

#[derive(Serialize)]
struct TestProofPayload {
    release_tag: String,
    repository: String,
    target: String,
    integrity_predicate: TestProofFile,
    attestation_bundle: TestProofFile,
    attestation_metadata: TestAttestationMetadataRef,
    asset_index: TestProofFile,
    public_sha256s: TestProofFile,
    checksum_sidecars: Vec<TestChecksumSidecarRef>,
    trust_policy: TestTrustPolicyRef,
    verifier_versions: TestVerifierVersions,
}

#[derive(Serialize)]
struct TestProofFile {
    path: String,
    sha256: String,
    size_bytes: u64,
}

#[derive(Serialize)]
struct TestAttestationMetadataRef {
    file: TestProofFile,
    signer_identity: String,
    issuer: String,
    keyset_id: String,
    predicate_sha256: String,
}

#[derive(Serialize)]
struct TestChecksumSidecarRef {
    path: String,
    sha256: String,
    subject: String,
}

#[derive(Serialize)]
struct TestTrustPolicyRef {
    path: String,
    identity: String,
    sha256: String,
}

#[derive(Serialize)]
struct TestVerifierVersions {
    m80_version: String,
    gh_version: String,
    release_integrity_schema_version: u32,
    asset_index_schema_version: u32,
}
