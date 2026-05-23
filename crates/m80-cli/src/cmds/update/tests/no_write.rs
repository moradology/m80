use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt as _};
use std::path::{Path, PathBuf};

use m80_image_manifest::{
    HostBinariesManifest, HostBinaryEntry, HostBinaryName, HostLaunchMaterialEntry,
    HostLaunchMaterialName, InstallProvenance, InstallProvenanceArtifact, InstallProvenanceRewrite,
    InstallProvenanceTransform,
};
use sha2::{Digest, Sha256};

use super::super::cmd_update;
use super::http_fixture::HttpFixture;
use super::proof_cache_fixture::write_proof_cache;
use crate::args::UpdateArgs;

#[test]
fn update_check_does_not_write_files_on_healthy_latest_status_path() {
    let fixture = HealthyInstallFixture::new();
    fixture.write_installed_release("v1.2.3");
    fixture.write_no_write_sentinels("v1.2.3");
    let latest_status = fixture.temp.path().join("latest-status.json");
    fs::write(
        &latest_status,
        super::status_artifact("v1.2.4", Some("v1.2.0"), &[]),
    )
    .expect("write latest status fixture");
    let before = snapshot_tree(fixture.temp.path());

    let status = cmd_update(fixture.update_args(Some(latest_status), None), false)
        .expect("update check should run");

    assert_eq!(status, 0);
    assert_eq!(snapshot_tree(fixture.temp.path()), before);
}

#[test]
fn update_check_fetches_only_latest_status_and_never_bundle_urls() {
    let fixture = HealthyInstallFixture::new();
    fixture.write_installed_release("v1.2.3");
    fixture.write_no_write_sentinels("v1.2.3");
    let server = HttpFixture::new();
    let status_body = status_artifact_with_bundle_urls(
        "v1.2.4",
        &server.url("/latest-bundle.tar.gz"),
        &server.url("/release-bundle.tar.gz"),
    );
    server.add_ok("/latest-status.json", status_body.into_bytes());
    let before = snapshot_tree(fixture.temp.path());

    let status = cmd_update(
        fixture.update_args(None, Some(server.url("/latest-status.json"))),
        false,
    )
    .expect("update check should run");

    assert_eq!(status, 0);
    assert_eq!(snapshot_tree(fixture.temp.path()), before);
    assert_eq!(server.requests(), vec!["/latest-status.json"]);
}

#[test]
fn update_check_uses_fallback_cache_without_rewriting_it() {
    let fixture = HealthyInstallFixture::new();
    fixture.write_installed_release("v1.2.3");
    fixture.write_no_write_sentinels("v1.2.3");
    let latest_status = fixture.temp.path().join("latest-status.json");
    fs::write(&latest_status, super::status_artifact("v1.2.3", None, &[]))
        .expect("write latest status cache fixture");
    let server = HttpFixture::new();
    let before = snapshot_tree(fixture.temp.path());

    let status = cmd_update(
        fixture.update_args(Some(latest_status), Some(server.url("/latest-status.json"))),
        false,
    )
    .expect("update check should run");

    assert_eq!(status, 0);
    assert_eq!(snapshot_tree(fixture.temp.path()), before);
    let requests = server.requests();
    assert!(!requests.is_empty(), "remote latest status should be tried");
    assert!(
        requests.iter().all(|path| path == "/latest-status.json"),
        "only the latest-status URL should be tried: {requests:?}"
    );
}

#[test]
fn update_output_exposes_remote_latest_status_source() {
    let source = "http://127.0.0.1/latest-status.json".to_owned();
    let metadata = crate::release_freshness::read_freshness_status_artifact_json(
        &status_artifact_with_bundle_urls(
            "v1.2.4",
            "http://127.0.0.1/latest-bundle.tar.gz",
            "http://127.0.0.1/release-bundle.tar.gz",
        ),
    )
    .expect("parse status artifact");
    let output = super::super::check_output(
        &super::active_report("v1.2.3"),
        super::super::LatestStatusInput::Available {
            source: source.clone(),
            metadata,
            origin: super::super::LatestStatusOrigin::Remote,
            offline_reason: None,
        },
        crate::release_freshness::UnixSeconds::new(super::timestamp("2026-05-21T12:30:00Z")),
    );

    assert_eq!(output.latest_status_source, source);
    assert_eq!(output.latest_status_error, None);
    assert_eq!(
        output.latest_status_origin,
        super::super::LatestStatusOrigin::Remote
    );
    assert_eq!(
        output.latest_status_cache_state,
        super::super::LatestStatusCacheState::NotUsed
    );
    let human = super::super::render_human(&output);
    assert!(human.contains(&format!("latest_status_source={source}\n")));
    assert!(human.contains("latest_status_origin=remote\n"));
    assert!(human.contains("latest_status_cache_state=not_used\n"));
    assert!(human.contains("latest_status_error=<unavailable>\n"));
    let json: serde_json::Value =
        serde_json::from_str(&crate::json::to_pretty(&output)).expect("update JSON parses");
    assert_eq!(json["data"]["latest_status_source"], source);
    assert_eq!(json["data"]["latest_status_origin"], "remote");
    assert_eq!(json["data"]["latest_status_cache_state"], "not_used");
    assert_eq!(json["data"]["latest_status_max_age_seconds"], 172_800);
    assert_eq!(json["data"]["safety_state"], "unknown");
    assert_eq!(json["data"]["safety_floor"]["metadata_source"], source);
    assert_eq!(
        json["data"]["safety_floor"]["published_at"],
        "2026-05-21T12:00:00Z"
    );
    assert!(json["data"]["latest_status_error"].is_null());
}

fn status_artifact_with_bundle_urls(tag: &str, latest_url: &str, release_url: &str) -> String {
    format!(
        r#"{{
          "schema_version":1,
          "freshness_network_bounded":true,
          "repository":"moradology/m80",
          "resolved_tag":"{tag}",
          "published_at":"2026-05-21T12:00:00Z",
          "fetch_policy":{{"connect_timeout_seconds":10,"max_time_seconds":120,"retry_count":2,"retry_delay_seconds":1}},
          "checked_urls":[{{"role":"latest-bundle","url":"{latest_url}","release_tag":"latest","asset_name":"m80-linux-x86_64.tar.gz","sources":["release-url-contract:latest-bundle"],"size_bytes":123,"sha256":"{}"}}],
          "public_assets":[{{"name":"m80-linux-x86_64.tar.gz","role":"bundle","url":"{release_url}","release_tag":"{tag}","size_bytes":123,"sha256":"{}"}}],
          "safety_floor":{}
        }}"#,
        "1".repeat(64),
        "2".repeat(64),
        super::safety_floor_json(None, &[], tag),
    )
}

struct HealthyInstallFixture {
    temp: tempfile::TempDir,
    _env_restore: m80_test_helpers::env::EnvRestore,
    _env_lock: std::sync::MutexGuard<'static, ()>,
}

impl HealthyInstallFixture {
    fn new() -> Self {
        let env_lock = crate::test_support::PROCESS_ENV_LOCK.lock().unwrap();
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

    fn bundle_cache_path(&self) -> PathBuf {
        self.temp.path().join("bundle-cache")
    }

    fn update_args(
        &self,
        latest_status: Option<PathBuf>,
        latest_status_url: Option<String>,
    ) -> UpdateArgs {
        UpdateArgs {
            check: true,
            install_root: self.install_root(),
            profile: None,
            latest_status,
            latest_status_url,
            config_path: Some(self.config_path()),
            profile_dir: Some(self.profile_dir()),
        }
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

    fn write_no_write_sentinels(&self, tag: &str) {
        fs::write(self.install_root().join("operator-sentinel"), "keep\n")
            .expect("write install-root sentinel");
        fs::write(self.profile_dir().join("operator-sentinel"), "keep\n")
            .expect("write profile-dir sentinel");
        fs::write(
            self.install_root()
                .join("versions")
                .join(tag)
                .join("artifacts/release-proof-cache/operator-sentinel"),
            "keep\n",
        )
        .expect("write proof-cache sentinel");
        fs::create_dir_all(self.bundle_cache_path()).expect("create bundle-cache sentinel dir");
        fs::write(self.bundle_cache_path().join("operator-sentinel"), "keep\n")
            .expect("write bundle-cache sentinel");
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
            entries.push(format!(
                "D\t{}\t{}",
                relative.display(),
                metadata.permissions().mode() & 0o777
            ));
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

fn sha256_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
