use std::ffi::OsString;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::symlink;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use super::{
    probe_run_root_reflink_at, verify_artifacts, ArtifactPreflightConfig, RunRootReflink,
    REQUIRED_STORAGE_HELPERS,
};
use crate::PreflightError;
use m80_image_manifest::{
    BuildReceipt, BuildReceiptArtifact, BuildReceiptArtifactKind, ImageKind, KernelKind, Manifest,
    ManifestError, RootfsFormat, SCHEMA_VERSION,
};

const SHA256_EMPTY: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

fn write_empty(path: &Path) {
    fs::write(path, b"").unwrap();
    #[cfg(unix)]
    fs::set_permissions(path, fs::Permissions::from_mode(0o644)).unwrap();
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

    let manifest = Manifest::new(
        daemon,
        SHA256_EMPTY.to_string(),
        "v1.15.1".to_string(),
        9001,
        ImageKind::Minimal,
        kernel.to_path_buf(),
        SHA256_EMPTY.to_string(),
        KernelKind::Stock,
        None,
        rootfs.clone(),
        SHA256_EMPTY.to_string(),
        "M80_READY".to_string(),
        RootfsFormat::Ext4,
        None,
        None,
    );
    let manifest_path = PathBuf::from(format!("{}.manifest.json", rootfs.display()));
    manifest.write(&manifest_path).unwrap();
    write_build_receipt(&rootfs, &manifest_path, &manifest);
    (rootfs, manifest)
}

fn sha256_file(path: &Path) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(fs::read(path).unwrap()))
}

fn write_build_receipt(rootfs: &Path, manifest_path: &Path, manifest: &Manifest) {
    let receipt_path = PathBuf::from(format!("{}.build-receipt.json", rootfs.display()));
    BuildReceipt::new(
        manifest_path.to_path_buf(),
        sha256_file(manifest_path),
        vec![
            BuildReceiptArtifact {
                kind: BuildReceiptArtifactKind::KernelImage,
                path: manifest.kernel_image.clone(),
                sha256: manifest.kernel_image_sha256.clone(),
            },
            BuildReceiptArtifact {
                kind: BuildReceiptArtifactKind::OutputRootfsImage,
                path: manifest.output_rootfs_image.clone(),
                sha256: manifest.output_rootfs_sha256.clone(),
            },
            BuildReceiptArtifact {
                kind: BuildReceiptArtifactKind::DaemonBinaryPath,
                path: manifest.daemon_binary_path.clone(),
                sha256: manifest.daemon_binary_sha256.clone(),
            },
        ],
    )
    .write(&receipt_path)
    .unwrap();
}

fn fixture_config() -> (
    tempfile::TempDir,
    tempfile::TempDir,
    ArtifactPreflightConfig,
) {
    let artifact_dir = tempfile::tempdir().unwrap();
    let helper_dir = tempfile::tempdir().unwrap();
    #[cfg(unix)]
    {
        fs::set_permissions(artifact_dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
        fs::set_permissions(helper_dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
    }
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

fn build_receipt_path(config: &ArtifactPreflightConfig) -> PathBuf {
    PathBuf::from(format!(
        "{}.build-receipt.json",
        config.rootfs_image.as_ref().unwrap().display()
    ))
}

#[test]
fn kernel_auto_discovery_picks_latest_vmlinux_entry() {
    let (_artifact_dir, _helper_dir, config) = fixture_config();

    let verified = verify_artifacts(&config, None).unwrap();

    assert_eq!(
        verified.kernel.file_name().unwrap(),
        std::ffi::OsStr::new("vmlinux-2027")
    );
}

#[test]
fn kernel_must_be_absolute() {
    let (_artifact_dir, _helper_dir, mut config) = fixture_config();
    config.kernel_image = Some(PathBuf::from("relative-vmlinux"));

    let err = verify_artifacts(&config, None).unwrap_err();

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

    let err = verify_artifacts(&config, None).unwrap_err();

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

    let err = verify_artifacts(&config, None).unwrap_err();

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

    let err = verify_artifacts(&config, None).unwrap_err();

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

    let err = verify_artifacts(&config, None).unwrap_err();

    match err {
        PreflightError::Manifest(ManifestError::Json(_)) => {}
        other => panic!("expected manifest JSON rejection, got {other:?}"),
    }
}

#[test]
fn manifest_sha256_mismatch_fails_closed() {
    let (_artifact_dir, _helper_dir, config) = fixture_config();
    fs::write(config.artifact_dir.join("vmlinux-2027"), b"tampered").unwrap();

    let err = verify_artifacts(&config, None).unwrap_err();

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

    let err = verify_artifacts(&config, None).unwrap_err();

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
fn cached_manifest_still_verifies_pinned_rootfs_sha256() {
    let (_artifact_dir, _helper_dir, config) = fixture_config();
    let manifest = Manifest::read(&manifest_path(&config)).unwrap();
    fs::write(config.rootfs_image.as_ref().unwrap(), b"tampered rootfs").unwrap();

    let err = verify_artifacts(&config, Some(&manifest)).unwrap_err();

    match err {
        PreflightError::Manifest(ManifestError::Sha256Mismatch { field, .. }) => {
            assert_eq!(field, "output_rootfs_image");
        }
        other => panic!("expected rootfs sha mismatch, got {other:?}"),
    }
}

#[test]
fn pinned_rootfs_fd_survives_path_replacement_after_preflight() {
    let (_artifact_dir, _helper_dir, config) = fixture_config();
    let verified = verify_artifacts(&config, None).unwrap();

    let replacement = config.artifact_dir.join("replacement-rootfs.ext4");
    fs::write(&replacement, b"replacement rootfs").unwrap();
    fs::rename(&replacement, config.rootfs_image.as_ref().unwrap()).unwrap();

    let pinned_bytes = fs::read(verified.pinned_rootfs.proc_fd_path()).unwrap();
    assert_eq!(pinned_bytes, b"");
}

#[cfg(unix)]
#[test]
fn rootfs_symlink_is_rejected_before_manifest_verification() {
    let (_artifact_dir, _helper_dir, mut config) = fixture_config();
    let link = config.artifact_dir.join("rootfs-link.ext4");
    symlink(config.rootfs_image.as_ref().unwrap(), &link).unwrap();
    config.rootfs_image = Some(link);

    let err = verify_artifacts(&config, None).unwrap_err();

    match err {
        PreflightError::PathIo { source, .. } => {
            assert_eq!(source.raw_os_error(), Some(nix::libc::ELOOP));
        }
        other => panic!("expected rootfs symlink rejection, got {other:?}"),
    }
}

#[cfg(unix)]
#[test]
fn group_writable_artifact_dir_fails_closed() {
    let (artifact_dir, _helper_dir, config) = fixture_config();
    fs::set_permissions(artifact_dir.path(), fs::Permissions::from_mode(0o775)).unwrap();

    let err = verify_artifacts(&config, None).unwrap_err();

    match err {
        PreflightError::ArtifactDirectoryWritable { path, mode } => {
            assert_eq!(path, artifact_dir.path());
            assert_eq!(mode, 0o775);
        }
        other => panic!("expected writable artifact dir rejection, got {other:?}"),
    }
}

#[cfg(unix)]
#[test]
fn group_writable_rootfs_file_fails_closed() {
    let (_artifact_dir, _helper_dir, config) = fixture_config();
    fs::set_permissions(
        config.rootfs_image.as_ref().unwrap(),
        fs::Permissions::from_mode(0o664),
    )
    .unwrap();

    let err = verify_artifacts(&config, None).unwrap_err();

    match err {
        PreflightError::ArtifactFileWritable { path, mode } => {
            assert_eq!(path, *config.rootfs_image.as_ref().unwrap());
            assert_eq!(mode, 0o664);
        }
        other => panic!("expected writable artifact file rejection, got {other:?}"),
    }
}

#[test]
fn missing_manifest_returns_typed_io_error() {
    let (_artifact_dir, _helper_dir, config) = fixture_config();
    let manifest_path = manifest_path(&config);
    fs::remove_file(&manifest_path).unwrap();

    let err = verify_artifacts(&config, None).unwrap_err();

    match err {
        PreflightError::Manifest(ManifestError::Io { path, source }) => {
            assert_eq!(path, manifest_path);
            assert_eq!(source.kind(), std::io::ErrorKind::NotFound);
        }
        other => panic!("expected missing manifest IO error, got {other:?}"),
    }
}

#[test]
fn missing_build_receipt_returns_typed_io_error() {
    let (_artifact_dir, _helper_dir, config) = fixture_config();
    let receipt_path = build_receipt_path(&config);
    fs::remove_file(&receipt_path).unwrap();

    let err = verify_artifacts(&config, None).unwrap_err();

    match err {
        PreflightError::BuildReceipt(ManifestError::Io { path, source }) => {
            assert_eq!(path, receipt_path);
            assert_eq!(source.kind(), std::io::ErrorKind::NotFound);
        }
        other => panic!("expected missing receipt IO error, got {other:?}"),
    }
}

#[test]
fn build_receipt_manifest_sha_mismatch_fails_preflight() {
    let (_artifact_dir, _helper_dir, config) = fixture_config();
    let receipt_path = build_receipt_path(&config);
    let mut receipt = BuildReceipt::read(&receipt_path).unwrap();
    receipt.manifest_sha256 = "0".repeat(64);
    receipt.write(&receipt_path).unwrap();

    let err = verify_artifacts(&config, None).unwrap_err();

    match err {
        PreflightError::BuildReceiptManifestMismatch { path, expected, .. } => {
            assert_eq!(path, manifest_path(&config));
            assert_eq!(expected, "0".repeat(64));
        }
        other => panic!("expected receipt manifest sha mismatch, got {other:?}"),
    }
}

#[test]
fn build_receipt_artifact_hash_must_match_manifest() {
    let (_artifact_dir, _helper_dir, config) = fixture_config();
    let receipt_path = build_receipt_path(&config);
    let mut receipt = BuildReceipt::read(&receipt_path).unwrap();
    receipt.artifacts[0].sha256 = "1".repeat(64);
    receipt.write(&receipt_path).unwrap();

    let err = verify_artifacts(&config, None).unwrap_err();

    match err {
        PreflightError::BuildReceiptArtifactHashMismatch { kind, actual, .. } => {
            assert_eq!(kind, BuildReceiptArtifactKind::KernelImage);
            assert_eq!(actual, "1".repeat(64));
        }
        other => panic!("expected receipt artifact hash mismatch, got {other:?}"),
    }
}

#[test]
fn run_root_must_already_exist() {
    let (_artifact_dir, _helper_dir, mut config) = fixture_config();
    config.run_root = config.artifact_dir.join("missing-run-root");

    let err = verify_artifacts(&config, None).unwrap_err();

    match err {
        PreflightError::RunRootUnavailable { reason } => {
            assert!(reason.contains("directory does not exist"), "{reason}");
        }
        other => panic!("expected run-root rejection, got {other:?}"),
    }
}

#[test]
fn run_root_reflink_probe_reports_supported_clone() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source");
    let dest = dir.path().join("dest");

    let result = probe_run_root_reflink_at(&source, &dest);

    if matches!(result, RunRootReflink::ProbeFailed { .. }) {
        panic!("probe should be conclusive when cp is available: {result:?}");
    }
    assert!(
        dest.exists() || matches!(result, RunRootReflink::Unsupported { .. }),
        "supported clone must create destination, unsupported clone must report fallback"
    );
    assert!(result.detail().contains("reflink") || result.detail().contains("overlay clone"));
}

#[test]
fn run_root_reflink_probe_failures_are_non_blocking() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("missing-parent").join("source");
    let dest = dir.path().join("dest");

    let result = probe_run_root_reflink_at(&source, &dest);

    assert!(matches!(result, RunRootReflink::ProbeFailed { .. }));
    assert!(result.detail().contains("probe inconclusive"));
}

#[test]
#[cfg(unix)]
fn run_root_reflink_probe_reports_full_copy_fallback_on_unsupported_clone() {
    let dir = tempfile::tempdir().unwrap();
    let cp = dir.path().join("cp");
    fs::write(
        &cp,
        b"#!/bin/sh\nprintf 'Operation not supported' >&2\nexit 1\n",
    )
    .unwrap();
    let mut perms = fs::metadata(&cp).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&cp, perms).unwrap();

    let source = dir.path().join("source");
    let dest = dir.path().join("dest");
    let result = super::probe_run_root_reflink_at_with_cp(&cp, &source, &dest);

    assert!(
        matches!(result, RunRootReflink::Unsupported { .. }),
        "failed reflink command should report full-copy fallback, got {result:?}"
    );
    assert!(result.detail().contains("full byte copy"));
    assert!(result.detail().contains("cp --reflink=never"));
}

#[test]
#[cfg(unix)]
fn run_root_reflink_probe_uses_configured_cp_path() {
    let run_root = tempfile::tempdir().unwrap();
    let helper_dir = tempfile::tempdir().unwrap();
    let cp = helper_dir.path().join("cp");
    fs::write(&cp, b"#!/bin/sh\nprintf configured-cp > \"$3\"\nexit 0\n").unwrap();
    let mut perms = fs::metadata(&cp).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&cp, perms).unwrap();

    let helper_path = helper_dir.path().as_os_str().to_owned();
    let result = super::probe_run_root_reflink(run_root.path(), Some(&helper_path));

    assert_eq!(result, RunRootReflink::Supported);
}

#[test]
fn storage_helpers_are_required_on_path() {
    let (_artifact_dir, _helper_dir, mut config) = fixture_config();
    config.helper_search_path = Some(OsString::from(""));

    let err = verify_artifacts(&config, None).unwrap_err();

    assert!(matches!(err, PreflightError::StorageHelperMissing(_)));
}

#[test]
fn kernel_kind_override_updates_verified_manifest() {
    let (_artifact_dir, _helper_dir, mut config) = fixture_config();
    config.kernel_kind = Some("stripped".to_string());

    let verified = verify_artifacts(&config, None).unwrap();

    assert_eq!(verified.manifest.kernel_kind, KernelKind::Stripped);
}
