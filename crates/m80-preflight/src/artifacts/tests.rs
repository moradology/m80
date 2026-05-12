use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use super::{verify_artifacts, ArtifactPreflightConfig, REQUIRED_STORAGE_HELPERS};
use crate::PreflightError;
use m80_image_manifest::{ImageKind, KernelKind, Manifest, ManifestError, SCHEMA_VERSION};

const SHA256_EMPTY: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

fn write_empty(path: &Path) {
    fs::write(path, b"").unwrap();
}

fn write_helpers(dir: &Path) -> OsString {
    for helper in REQUIRED_STORAGE_HELPERS {
        write_empty(&dir.join(helper));
    }
    dir.as_os_str().to_owned()
}

fn existing_run_root() -> PathBuf {
    std::env::current_dir().unwrap()
}

fn fixture_manifest(dir: &Path, kernel: &Path) -> (PathBuf, Manifest) {
    let rootfs = dir.join("rootfs.ext4");
    let daemon = dir.join("m80-guestd");
    write_empty(&rootfs);
    write_empty(&daemon);

    let manifest = Manifest {
        daemon_binary_path: daemon,
        daemon_binary_sha256: SHA256_EMPTY.to_string(),
        expected_firecracker_version: "v1.15.1".to_string(),
        guest_port: 9001,
        image_kind: ImageKind::Minimal,
        kernel_image: kernel.to_path_buf(),
        kernel_image_sha256: SHA256_EMPTY.to_string(),
        kernel_kind: KernelKind::Stock,
        no_egress_reason: None,
        output_rootfs_image: rootfs.clone(),
        output_rootfs_sha256: SHA256_EMPTY.to_string(),
        ready_marker: "M80_READY".to_string(),
        schema_version: SCHEMA_VERSION,
        source_rootfs_image: None,
        source_rootfs_sha256: None,
    };
    let manifest_path = PathBuf::from(format!("{}.manifest.json", rootfs.display()));
    manifest.write(&manifest_path).unwrap();
    (rootfs, manifest)
}

fn fixture_config() -> (
    tempfile::TempDir,
    tempfile::TempDir,
    ArtifactPreflightConfig,
) {
    let artifact_dir = tempfile::tempdir().unwrap();
    let helper_dir = tempfile::tempdir().unwrap();
    let older_kernel = artifact_dir.path().join("vmlinux-2026");
    let newer_kernel = artifact_dir.path().join("vmlinux-2027");
    write_empty(&older_kernel);
    write_empty(&newer_kernel);
    let (rootfs, _) = fixture_manifest(artifact_dir.path(), &newer_kernel);

    let config = ArtifactPreflightConfig {
        kernel_image: None,
        artifact_dir: artifact_dir.path().to_path_buf(),
        rootfs_image: Some(rootfs),
        kernel_kind: None,
        run_root: existing_run_root(),
        helper_search_path: Some(write_helpers(helper_dir.path())),
    };
    (artifact_dir, helper_dir, config)
}

fn manifest_path(config: &ArtifactPreflightConfig) -> PathBuf {
    PathBuf::from(format!(
        "{}.manifest.json",
        config.rootfs_image.as_ref().unwrap().display()
    ))
}

#[test]
fn kernel_auto_discovery_picks_latest_vmlinux_entry() {
    let (_artifact_dir, _helper_dir, config) = fixture_config();

    let verified = verify_artifacts(&config).unwrap();

    assert_eq!(
        verified.kernel.file_name().unwrap(),
        std::ffi::OsStr::new("vmlinux-2027")
    );
}

#[test]
fn kernel_must_be_absolute() {
    let (_artifact_dir, _helper_dir, mut config) = fixture_config();
    config.kernel_image = Some(PathBuf::from("relative-vmlinux"));

    let err = verify_artifacts(&config).unwrap_err();

    match err {
        PreflightError::NonAbsolutePath { kind, path } => {
            assert_eq!(kind, "kernel");
            assert_eq!(path, PathBuf::from("relative-vmlinux"));
        }
        other => panic!("expected kernel non-absolute error, got {other:?}"),
    }
}

#[test]
fn rootfs_must_be_absolute() {
    let (_artifact_dir, _helper_dir, mut config) = fixture_config();
    config.rootfs_image = Some(PathBuf::from("relative-rootfs.ext4"));

    let err = verify_artifacts(&config).unwrap_err();

    match err {
        PreflightError::NonAbsolutePath { kind, path } => {
            assert_eq!(kind, "rootfs");
            assert_eq!(path, PathBuf::from("relative-rootfs.ext4"));
        }
        other => panic!("expected rootfs non-absolute error, got {other:?}"),
    }
}

#[test]
fn manifest_schema_version_must_match_current_schema() {
    let (_artifact_dir, _helper_dir, config) = fixture_config();
    let manifest_path = manifest_path(&config);
    let raw = fs::read_to_string(&manifest_path).unwrap();
    fs::write(
        &manifest_path,
        raw.replace(
            &format!("\"schema_version\": {SCHEMA_VERSION}"),
            "\"schema_version\": 2",
        ),
    )
    .unwrap();

    let err = verify_artifacts(&config).unwrap_err();

    match err {
        PreflightError::Manifest(ManifestError::UnsupportedSchemaVersion(2)) => {}
        other => panic!("expected schema version rejection, got {other:?}"),
    }
}

#[test]
fn manifest_future_schema_version_returns_typed_error() {
    let (_artifact_dir, _helper_dir, config) = fixture_config();
    let manifest_path = manifest_path(&config);
    let raw = fs::read_to_string(&manifest_path).unwrap();
    fs::write(
        &manifest_path,
        raw.replace(
            &format!("\"schema_version\": {SCHEMA_VERSION}"),
            "\"schema_version\": 99",
        ),
    )
    .unwrap();

    let err = verify_artifacts(&config).unwrap_err();

    match err {
        PreflightError::Manifest(ManifestError::UnsupportedSchemaVersion(99)) => {}
        other => panic!("expected schema version rejection, got {other:?}"),
    }
}

#[test]
fn manifest_unknown_field_rejected_at_preflight() {
    let (_artifact_dir, _helper_dir, config) = fixture_config();
    let manifest_path = manifest_path(&config);
    let raw = fs::read_to_string(&manifest_path).unwrap();
    let mutated = raw.replace("\n}", ",\n  \"future_field\": \"surprise\"\n}");
    fs::write(&manifest_path, mutated).unwrap();

    let err = verify_artifacts(&config).unwrap_err();

    match err {
        PreflightError::Manifest(ManifestError::Json(_)) => {}
        other => panic!("expected manifest JSON rejection, got {other:?}"),
    }
}

#[test]
fn manifest_sha256_mismatch_fails_closed() {
    let (_artifact_dir, _helper_dir, config) = fixture_config();
    fs::write(config.artifact_dir.join("vmlinux-2027"), b"tampered").unwrap();

    let err = verify_artifacts(&config).unwrap_err();

    match err {
        PreflightError::Manifest(ManifestError::Sha256Mismatch { field, .. }) => {
            assert_eq!(field, "kernel_image");
        }
        other => panic!("expected sha mismatch, got {other:?}"),
    }
}

#[test]
fn manifest_sha_mismatch_fails_preflight() {
    let (_artifact_dir, _helper_dir, config) = fixture_config();
    fs::write(config.rootfs_image.as_ref().unwrap(), b"tampered rootfs").unwrap();

    let err = verify_artifacts(&config).unwrap_err();

    match err {
        PreflightError::Manifest(ManifestError::Sha256Mismatch {
            field,
            expected,
            actual,
        }) => {
            assert_eq!(field, "output_rootfs_image");
            assert_eq!(expected, SHA256_EMPTY);
            assert_ne!(actual, expected);
        }
        other => panic!("expected rootfs sha mismatch, got {other:?}"),
    }
}

#[test]
fn missing_manifest_returns_typed_io_error() {
    let (_artifact_dir, _helper_dir, config) = fixture_config();
    let manifest_path = manifest_path(&config);
    fs::remove_file(&manifest_path).unwrap();

    let err = verify_artifacts(&config).unwrap_err();

    match err {
        PreflightError::Manifest(ManifestError::Io { path, source }) => {
            assert_eq!(path, manifest_path);
            assert_eq!(source.kind(), std::io::ErrorKind::NotFound);
        }
        other => panic!("expected missing manifest IO error, got {other:?}"),
    }
}

#[test]
fn run_root_must_already_exist() {
    let (_artifact_dir, _helper_dir, mut config) = fixture_config();
    config.run_root = config.artifact_dir.join("missing-run-root");

    let err = verify_artifacts(&config).unwrap_err();

    match err {
        PreflightError::RunRootUnavailable { reason } => {
            assert!(reason.contains("directory does not exist"), "{reason}");
        }
        other => panic!("expected run-root rejection, got {other:?}"),
    }
}

#[test]
fn storage_helpers_are_required_on_path() {
    let (_artifact_dir, _helper_dir, mut config) = fixture_config();
    config.helper_search_path = Some(OsString::from(""));

    let err = verify_artifacts(&config).unwrap_err();

    assert!(matches!(err, PreflightError::StorageHelperMissing(_)));
}

#[test]
fn kernel_kind_override_updates_verified_manifest() {
    let (_artifact_dir, _helper_dir, mut config) = fixture_config();
    config.kernel_kind = Some("stripped".to_string());

    let verified = verify_artifacts(&config).unwrap();

    assert_eq!(verified.manifest.kernel_kind, KernelKind::Stripped);
}
