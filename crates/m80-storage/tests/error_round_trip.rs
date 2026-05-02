//! Verify that each `StorageError` variant displays sensibly via `Display`.

use std::path::PathBuf;

use m80_storage::StorageError;

#[test]
fn mkfs_error_displays() {
    let e = StorageError::Mkfs(std::io::Error::other("device busy"));
    let s = format!("{e}");
    assert!(s.contains("mkfs"), "expected 'mkfs' in: {s}");
}

#[test]
fn e2fsck_failed_displays_exit_and_stderr() {
    let e = StorageError::E2fsckFailed {
        exit: 8,
        stderr: "filesystem corrupted".into(),
    };
    let s = format!("{e}");
    assert!(s.contains("8"), "expected exit code in: {s}");
    assert!(s.contains("filesystem corrupted"), "expected stderr in: {s}");
}

#[test]
fn admissibility_refused_displays() {
    let e = StorageError::AdmissibilityRefused;
    let s = format!("{e}");
    assert!(!s.is_empty());
    assert!(s.contains("admissibility"));
}

#[test]
fn swap_failed_displays() {
    let e = StorageError::SwapFailed;
    let s = format!("{e}");
    assert!(!s.is_empty());
    assert!(s.contains("swap") || s.contains("atomic"));
}

#[test]
fn io_error_displays_path_and_source() {
    let e = StorageError::Io {
        path: PathBuf::from("/var/run/m80/vm-001/rootfs.ext4"),
        source: std::io::Error::from(std::io::ErrorKind::PermissionDenied),
    };
    let s = format!("{e}");
    assert!(s.contains("rootfs.ext4"), "path must appear: {s}");
}

#[test]
fn io_error_carries_path_in_variant() {
    let expected_path = PathBuf::from("/some/path");
    let e = StorageError::Io {
        path: expected_path.clone(),
        source: std::io::Error::other("boom"),
    };
    match e {
        StorageError::Io { path, .. } => assert_eq!(path, expected_path),
        other => panic!("unexpected: {other:?}"),
    }
}
