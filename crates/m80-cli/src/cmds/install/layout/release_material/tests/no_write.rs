use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::symlink;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use m80_firecracker::FcError;
use m80_image_manifest::{
    BuildReceipt, BuildReceiptArtifact, BuildReceiptArtifactKind, ImageKind, KernelKind, Manifest,
    RootfsFormat, BUILD_RECEIPT_SCHEMA_VERSION, INSTALL_PROVENANCE_SCHEMA_VERSION,
};
use serde_json::json;
use sha2::{Digest, Sha256};

use super::super::test_env::{
    fake_gh_fixture, official_release_plan, official_release_plan_for_tag, write_fake_curl,
    EnvVarGuard,
};
use super::super::test_fixture::{
    write_direct_release_materials_with, write_direct_release_materials_with_bundle_bytes,
    ReleaseFixtureOptions,
};

mod reinstall;
mod release_verifier_matrix;

#[test]
fn upgrade_install_replaces_active_after_new_release_is_fully_committed() {
    let _guard = super::super::super::INSTALL_PREFLIGHT_ENV_LOCK
        .lock()
        .unwrap();
    let temp = tempfile::tempdir().unwrap();
    let install_root = temp.path().join("install-root");

    let first = install_layout_success_with_installable_bundle(&install_root, "v0.0.0");
    assert_eq!(first.release_tag, "v0.0.0");
    assert!(first.active_pointer_flipped);
    let previous_active = fs::read_link(install_root.join("active")).unwrap();
    let previous_bundle = fs::read(previous_active.join("bundle.json")).unwrap();
    let previous_profile = fs::read_to_string(install_root.join("profiles/default.toml")).unwrap();
    assert!(
        previous_profile.contains("description = 'm80 installed default profile'"),
        "first install should create an install-owned profile: {previous_profile}"
    );

    let second = install_layout_success_with_installable_bundle(&install_root, "v0.0.1");

    assert_eq!(second.release_tag, "v0.0.1");
    assert!(second.active_pointer_flipped);
    assert_eq!(
        second.previous_active_version_dir.as_deref(),
        Some(previous_active.to_str().unwrap())
    );
    assert_eq!(
        second.previous_active_release_tag.as_deref(),
        Some("v0.0.0")
    );
    assert_eq!(
        second.finalization_order.last().copied(),
        Some("active_pointer_flip")
    );
    assert_eq!(
        fs::read_link(install_root.join("active")).unwrap(),
        install_root.join("versions/v0.0.1")
    );
    assert_eq!(
        fs::read(previous_active.join("bundle.json")).unwrap(),
        previous_bundle,
        "upgrade must leave the previous version directory intact"
    );
    let upgraded_profile = fs::read_to_string(install_root.join("profiles/default.toml")).unwrap();
    assert!(
        upgraded_profile.contains("release_tag = 'v0.0.1'"),
        "upgrade should move the install-owned default profile to the new release: {upgraded_profile}"
    );
    assert!(
        install_root.join("versions/v0.0.0/bin/m80").exists(),
        "previous active version remains available for explicit rollback"
    );
    assert_no_layout_staging_dirs(&install_root, "successful upgrade");
}

#[test]
fn proof_cache_write_failure_leaves_previous_active_profile_and_config_selected() {
    assert_proof_cache_failure_preserves_active_state(ProofCacheFailureScenario {
        name: "write failure",
        env_key: "M80_INSTALL_INJECT_PROOF_CACHE_WRITE_FAILURE",
        expected: "injected proof-cache write failure",
    });
}

fn install_layout_success_with_installable_bundle(
    install_root: &Path,
    release_tag: &'static str,
) -> super::super::super::LayoutInstallSummary {
    let temp = tempfile::tempdir().unwrap();
    let material_dir = temp.path().join("materials");
    fs::create_dir(&material_dir).unwrap();
    let log_path = temp.path().join("curl.log");
    let bin_dir = temp.path().join("bin");
    fs::create_dir(&bin_dir).unwrap();
    write_fake_curl(&bin_dir);
    let bundle_bytes = write_installable_bundle_bytes_for_tag(temp.path(), release_tag);
    let fixture = write_direct_release_materials_with_bundle_bytes(
        &material_dir,
        ReleaseFixtureOptions {
            release_tag: Some(release_tag),
            ..ReleaseFixtureOptions::default()
        },
        &bundle_bytes,
    );

    let _path_env = EnvVarGuard::prepend_paths(&[install_root.join("bin"), bin_dir]);
    let _material_env = EnvVarGuard::set("M80_FAKE_CURL_MATERIAL_DIR", &material_dir);
    let _log_env = EnvVarGuard::set("M80_FAKE_CURL_LOG", &log_path);
    let _gh_env = EnvVarGuard::set(
        "M80_RELEASE_ATTESTATION_GH",
        &fake_gh_fixture("fake-gh-attestation-supported.sh"),
    );
    let _expect_source_digest = EnvVarGuard::set_value(
        "M80_FAKE_GH_EXPECT_SOURCE_DIGEST",
        "0123456789abcdef0123456789abcdef01234567",
    );
    let _hostless_preflight = EnvVarGuard::set_value("M80_INSTALL_TEST_HOSTLESS_OFFICIAL", "1");

    super::super::super::install_bundle_layout(&official_release_plan_for_tag(
        install_root,
        release_tag,
    ))
    .unwrap_or_else(|err| {
        panic!(
            "installable fixture should install verified bundle {}: {err}",
            fixture.bundle_url
        )
    })
}

#[test]
fn proof_cache_manifest_digest_failure_leaves_previous_active_profile_and_config_selected() {
    assert_proof_cache_failure_preserves_active_state(ProofCacheFailureScenario {
        name: "manifest digest failure",
        env_key: "M80_INSTALL_INJECT_PROOF_CACHE_DIGEST_FAILURE",
        expected: "manifest_digest mismatch",
    });
}

#[test]
fn proof_cache_mode_failure_leaves_previous_active_profile_and_config_selected() {
    assert_proof_cache_failure_preserves_active_state(ProofCacheFailureScenario {
        name: "mode failure",
        env_key: "M80_INSTALL_INJECT_PROOF_CACHE_MODE_FAILURE",
        expected: "proof-cache mode mismatch",
    });
}

#[derive(Clone, Copy)]
struct ProofCacheFailureScenario {
    name: &'static str,
    env_key: &'static str,
    expected: &'static str,
}

fn assert_proof_cache_failure_preserves_active_state(scenario: ProofCacheFailureScenario) {
    let temp = tempfile::tempdir().unwrap();
    let install_root = temp.path().join("install-root");
    seed_install_root_snapshot(&install_root);
    let previous = fs::read_link(install_root.join("active")).unwrap();
    let previous_profile = fs::read(install_root.join("profiles/default.toml")).unwrap();
    let previous_config = fs::read(install_root.join("config.toml")).unwrap();
    let attempted_version = install_root.join("versions/v0.0.0");
    let (err, _log) =
        install_layout_error_with_installable_bundle_and_env(&install_root, scenario.env_key);

    let message = err.to_string();
    assert!(
        message.contains(scenario.expected),
        "{} missing {:?}: {message}",
        scenario.name,
        scenario.expected
    );
    assert_eq!(
        fs::read_link(install_root.join("active")).unwrap(),
        previous,
        "{} changed active pointer before proof cache was verified",
        scenario.name
    );
    assert_eq!(
        fs::read(install_root.join("profiles/default.toml")).unwrap(),
        previous_profile,
        "{} changed default profile before proof cache was verified",
        scenario.name
    );
    assert_eq!(
        fs::read(install_root.join("config.toml")).unwrap(),
        previous_config,
        "{} changed config before proof cache was verified",
        scenario.name
    );
    assert!(
        !attempted_version.exists(),
        "{} published the attempted version before proof cache was verified",
        scenario.name
    );
    assert!(
        !install_root.join("run").exists(),
        "{} created runtime state before proof cache was verified",
        scenario.name
    );
    assert_no_layout_staging_dirs(&install_root, scenario.name);
}

fn install_layout_error_with_curl_log(
    install_root: &Path,
    options: ReleaseFixtureOptions,
) -> (FcError, String) {
    let _guard = super::super::super::INSTALL_PREFLIGHT_ENV_LOCK
        .lock()
        .unwrap();
    let temp = tempfile::tempdir().unwrap();
    let material_dir = temp.path().join("materials");
    fs::create_dir(&material_dir).unwrap();
    let log_path = temp.path().join("curl.log");
    let bin_dir = temp.path().join("bin");
    fs::create_dir(&bin_dir).unwrap();
    write_fake_curl(&bin_dir);
    let fixture = write_direct_release_materials_with(&material_dir, options);

    let _path_env = EnvVarGuard::prepend_paths(&[install_root.join("bin"), bin_dir]);
    let _material_env = EnvVarGuard::set("M80_FAKE_CURL_MATERIAL_DIR", &material_dir);
    let _log_env = EnvVarGuard::set("M80_FAKE_CURL_LOG", &log_path);
    let _gh_env = EnvVarGuard::set(
        "M80_RELEASE_ATTESTATION_GH",
        &fake_gh_fixture("fake-gh-attestation-supported.sh"),
    );
    let _expect_source_digest = EnvVarGuard::set_value(
        "M80_FAKE_GH_EXPECT_SOURCE_DIGEST",
        "0123456789abcdef0123456789abcdef01234567",
    );
    let _gh_failure = options
        .gh_failure
        .then(|| EnvVarGuard::set_value("M80_FAKE_GH_FAIL", "1"));
    let _gh_omit_subject = options
        .gh_omit_subject
        .then(|| EnvVarGuard::set_value("M80_FAKE_GH_OMIT_SUBJECT", "1"));
    let _gh_wrong_subject_digest = options
        .gh_wrong_subject_digest
        .then(|| EnvVarGuard::set_value("M80_FAKE_GH_WRONG_SUBJECT_DIGEST", "1"));
    let _gh_wrong_source_ref = options
        .gh_wrong_source_ref
        .then(|| EnvVarGuard::set_value("M80_FAKE_GH_EXPECT_SOURCE_REF", "refs/tags/v9.9.9"));

    let err = match super::super::super::install_bundle_layout(&official_release_plan(install_root))
    {
        Ok(_) => {
            panic!(
                "scenario unexpectedly installed verified bundle: {}",
                fixture.bundle_url
            )
        }
        Err(err) => err,
    };
    let log = fs::read_to_string(&log_path).unwrap_or_default();
    (err, log)
}

fn install_layout_error_with_installable_bundle_and_env(
    install_root: &Path,
    env_key: &'static str,
) -> (FcError, String) {
    let _guard = super::super::super::INSTALL_PREFLIGHT_ENV_LOCK
        .lock()
        .unwrap();
    let temp = tempfile::tempdir().unwrap();
    let material_dir = temp.path().join("materials");
    fs::create_dir(&material_dir).unwrap();
    let log_path = temp.path().join("curl.log");
    let bin_dir = temp.path().join("bin");
    fs::create_dir(&bin_dir).unwrap();
    write_fake_curl(&bin_dir);
    let bundle_bytes = write_installable_bundle_bytes(temp.path());
    let fixture = write_direct_release_materials_with_bundle_bytes(
        &material_dir,
        ReleaseFixtureOptions::default(),
        &bundle_bytes,
    );

    let _path_env = EnvVarGuard::prepend_paths(&[install_root.join("bin"), bin_dir]);
    let _material_env = EnvVarGuard::set("M80_FAKE_CURL_MATERIAL_DIR", &material_dir);
    let _log_env = EnvVarGuard::set("M80_FAKE_CURL_LOG", &log_path);
    let _gh_env = EnvVarGuard::set(
        "M80_RELEASE_ATTESTATION_GH",
        &fake_gh_fixture("fake-gh-attestation-supported.sh"),
    );
    let _expect_source_digest = EnvVarGuard::set_value(
        "M80_FAKE_GH_EXPECT_SOURCE_DIGEST",
        "0123456789abcdef0123456789abcdef01234567",
    );
    let _failure_env = EnvVarGuard::set_value(env_key, "1");

    let err = match super::super::super::install_bundle_layout(&official_release_plan(install_root))
    {
        Ok(_) => {
            panic!(
                "scenario unexpectedly installed verified bundle: {}",
                fixture.bundle_url
            )
        }
        Err(err) => err,
    };
    let log = fs::read_to_string(&log_path).unwrap_or_default();
    (err, log)
}

#[derive(Debug, PartialEq, Eq)]
enum InstallSnapshotEntry {
    Dir,
    File(Vec<u8>),
    Symlink(PathBuf),
}

fn seed_install_root_snapshot(install_root: &Path) -> BTreeMap<PathBuf, InstallSnapshotEntry> {
    let previous = install_root.join("versions/v-previous");
    fs::create_dir_all(&previous).unwrap();
    fs::write(previous.join("marker"), b"previous install\n").unwrap();
    fs::create_dir_all(install_root.join("profiles")).unwrap();
    fs::write(
        install_root.join("profiles/default.toml"),
        b"description = \"old profile\"\n",
    )
    .unwrap();
    fs::write(
        install_root.join("config.toml"),
        b"default_profile = 'old'\n",
    )
    .unwrap();
    let stale = install_root.join(".staging/layout-stale");
    fs::create_dir_all(&stale).unwrap();
    fs::write(stale.join("partial"), b"stale partial\n").unwrap();
    symlink(&previous, install_root.join("active")).unwrap();
    snapshot_install_root(install_root)
}

const REQUIRED_INSTALLABLE_BUNDLE_FILES: &[&str] = &[
    "bin/m80",
    "bin/m80-jailer-harden",
    "bin/m80-net-helper",
    "artifacts/vmlinux",
    "artifacts/output.ext4",
    "artifacts/output.ext4.manifest.json",
    "artifacts/output.ext4.build-receipt.json",
    "artifacts/m80-guestd",
    "install.sh",
    "bundle.json",
    "SHA256SUMS",
];

const INSTALLABLE_PAYLOAD_FILES: &[&str] = &[
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

fn write_installable_bundle_bytes(root: &Path) -> Vec<u8> {
    write_installable_bundle_bytes_for_tag(root, "v0.0.0")
}

fn write_installable_bundle_bytes_for_tag(root: &Path, release_tag: &str) -> Vec<u8> {
    let src = root.join("installable-src");
    fs::create_dir_all(src.join("bin")).unwrap();
    fs::create_dir_all(src.join("artifacts")).unwrap();
    write_executable(
        &src.join("bin/m80"),
        &format!(
            "#!/bin/sh\nif [ \"$1\" = --version ]; then printf 'm80 {}\\n'; exit 0; fi\n",
            release_tag.trim_start_matches('v')
        ),
    );
    write_executable(
        &src.join("bin/m80-jailer-harden"),
        "#!/bin/sh\nif [ \"$1\" = --version ]; then printf 'm80-jailer-harden 0.0.0\\n'; exit 0; fi\n",
    );
    write_executable(
        &src.join("bin/m80-net-helper"),
        "#!/bin/sh\nif [ \"$1\" = --version ]; then printf 'm80-net-helper 0.0.0\\n'; exit 0; fi\n",
    );
    fs::write(src.join("artifacts/vmlinux"), b"kernel").unwrap();
    fs::write(src.join("artifacts/output.ext4"), b"rootfs").unwrap();
    fs::write(src.join("artifacts/m80-guestd"), b"guestd").unwrap();
    fs::write(
        src.join("install.sh"),
        b"#!/bin/sh\nexec ./bin/m80 install \"$@\"\n",
    )
    .unwrap();

    let stale_artifacts = PathBuf::from("/tmp/m80-release-bundle/artifacts");
    write_manifest(&src, &stale_artifacts);
    let manifest_path = stale_artifacts.join("output.ext4.manifest.json");
    write_build_receipt(&src, &stale_artifacts, &manifest_path);
    write_bundle_metadata(&src, &manifest_path, release_tag);
    write_sha256s(&src);
    set_installable_bundle_modes(&src);

    let tarball = root.join("m80-linux-x86_64.tar.gz");
    let output = Command::new("tar")
        .arg("-czf")
        .arg(&tarball)
        .arg("-C")
        .arg(&src)
        .args(REQUIRED_INSTALLABLE_BUNDLE_FILES)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "tar failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    fs::read(tarball).unwrap()
}

fn write_manifest(src: &Path, stale_artifacts: &Path) {
    let manifest = Manifest::new(
        stale_artifacts.join("m80-guestd"),
        sha256_hex(&src.join("artifacts/m80-guestd")),
        "v1.15.1".to_owned(),
        m80_proto::GUEST_PORT_DEFAULT,
        ImageKind::Minimal,
        stale_artifacts.join("vmlinux"),
        sha256_hex(&src.join("artifacts/vmlinux")),
        KernelKind::Stock,
        Some(m80_image_manifest::DEFAULT_NO_EGRESS_REASON.to_owned()),
        stale_artifacts.join("output.ext4"),
        sha256_hex(&src.join("artifacts/output.ext4")),
        m80_proto::READY_MARKER_DEFAULT.to_owned(),
        RootfsFormat::Ext4,
        None,
        None,
    );
    manifest
        .write(&src.join("artifacts/output.ext4.manifest.json"))
        .unwrap();
}

fn write_build_receipt(src: &Path, stale_artifacts: &Path, manifest_path: &Path) {
    let receipt = BuildReceipt::new(
        manifest_path.to_path_buf(),
        sha256_hex(&src.join("artifacts/output.ext4.manifest.json")),
        vec![
            BuildReceiptArtifact {
                kind: BuildReceiptArtifactKind::KernelImage,
                path: stale_artifacts.join("vmlinux"),
                sha256: sha256_hex(&src.join("artifacts/vmlinux")),
            },
            BuildReceiptArtifact {
                kind: BuildReceiptArtifactKind::OutputRootfsImage,
                path: stale_artifacts.join("output.ext4"),
                sha256: sha256_hex(&src.join("artifacts/output.ext4")),
            },
            BuildReceiptArtifact {
                kind: BuildReceiptArtifactKind::DaemonBinaryPath,
                path: stale_artifacts.join("m80-guestd"),
                sha256: sha256_hex(&src.join("artifacts/m80-guestd")),
            },
        ],
    );
    receipt
        .write(&src.join("artifacts/output.ext4.build-receipt.json"))
        .unwrap();
}

fn write_bundle_metadata(src: &Path, manifest_path: &Path, release_tag: &str) {
    let files = INSTALLABLE_PAYLOAD_FILES
        .iter()
        .map(|path| {
            let full = src.join(path);
            json!({
                "path": path,
                "sha256": sha256_hex(&full),
                "size_bytes": full.metadata().unwrap().len(),
            })
        })
        .collect::<Vec<_>>();
    let metadata = json!({
        "schema_version": 1,
        "release_tag": release_tag,
        "m80_version": release_tag,
        "package_version": env!("CARGO_PKG_VERSION"),
        "target": "linux-x86_64",
        "os": "linux",
        "arch": "x86_64",
        "image_kind": "minimal",
        "m80_protocol_version": m80_proto::PROTOCOL_VERSION,
        "guestd_package_version": env!("CARGO_PKG_VERSION"),
        "guest_protocol_version": m80_proto::PROTOCOL_VERSION,
        "manifest_schema_version": m80_image_manifest::SCHEMA_VERSION,
        "build_receipt_schema_version": BUILD_RECEIPT_SCHEMA_VERSION,
        "build_receipt_manifest_path": manifest_path.display().to_string(),
        "install_provenance_schema_version": INSTALL_PROVENANCE_SCHEMA_VERSION,
        "install_provenance_required": true,
        "expected_firecracker_version": "v1.15.1",
        "files": files,
    });
    let mut encoded = serde_json::to_string_pretty(&metadata).unwrap();
    encoded.push('\n');
    fs::write(src.join("bundle.json"), encoded).unwrap();
}

fn write_sha256s(src: &Path) {
    let mut lines = String::new();
    for relpath in INSTALLABLE_PAYLOAD_FILES
        .iter()
        .copied()
        .chain(["bundle.json"])
    {
        lines.push_str(&format!("{}  {relpath}\n", sha256_hex(&src.join(relpath))));
    }
    fs::write(src.join("SHA256SUMS"), lines).unwrap();
}

fn write_executable(path: &Path, body: &str) {
    fs::write(path, body).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn set_installable_bundle_modes(src: &Path) {
    for relpath in [
        "bin/m80",
        "bin/m80-jailer-harden",
        "bin/m80-net-helper",
        "install.sh",
    ] {
        fs::set_permissions(src.join(relpath), fs::Permissions::from_mode(0o755)).unwrap();
    }
    for relpath in [
        "artifacts/vmlinux",
        "artifacts/output.ext4",
        "artifacts/output.ext4.manifest.json",
        "artifacts/output.ext4.build-receipt.json",
        "artifacts/m80-guestd",
        "bundle.json",
        "SHA256SUMS",
    ] {
        fs::set_permissions(src.join(relpath), fs::Permissions::from_mode(0o644)).unwrap();
    }
}

fn sha256_hex(path: &Path) -> String {
    let bytes = fs::read(path).unwrap();
    format!("{:x}", Sha256::digest(&bytes))
}

fn snapshot_install_root(root: &Path) -> BTreeMap<PathBuf, InstallSnapshotEntry> {
    let mut entries = BTreeMap::new();
    if root.exists() {
        capture_snapshot(root, Path::new(""), &mut entries);
    }
    entries
}

fn assert_no_layout_staging_dirs(install_root: &Path, scenario_name: &str) {
    let staging_parent = install_root.join(".staging");
    if !staging_parent.exists() {
        return;
    }
    let leaked = fs::read_dir(&staging_parent)
        .unwrap()
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with("layout-"))
        .collect::<Vec<_>>();
    assert!(
        leaked.is_empty(),
        "{scenario_name} leaked staging directories before proof cache was verified: {leaked:?}"
    );
}

fn capture_snapshot(
    path: &Path,
    relative: &Path,
    entries: &mut BTreeMap<PathBuf, InstallSnapshotEntry>,
) {
    let metadata = fs::symlink_metadata(path).unwrap();
    if metadata.file_type().is_symlink() {
        entries.insert(
            relative.to_path_buf(),
            InstallSnapshotEntry::Symlink(fs::read_link(path).unwrap()),
        );
    } else if metadata.is_dir() {
        if !relative.as_os_str().is_empty() {
            entries.insert(relative.to_path_buf(), InstallSnapshotEntry::Dir);
        }
        for entry in fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            capture_snapshot(&entry.path(), &relative.join(entry.file_name()), entries);
        }
    } else {
        entries.insert(
            relative.to_path_buf(),
            InstallSnapshotEntry::File(fs::read(path).unwrap()),
        );
    }
}
