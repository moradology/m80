//! Verify that each `StorageError` variant displays sensibly via `Display`.

use std::path::PathBuf;

use m80_storage::StorageError;

#[test]
fn overlay_create_failed_displays_path() {
    let e = StorageError::OverlayCreateFailed {
        path: PathBuf::from("/run/m80/vm-1/overlay.ext4"),
        err: std::io::Error::from(std::io::ErrorKind::NotFound),
    };
    let s = format!("{e}");
    assert!(s.contains("overlay.ext4"), "path must appear: {s}");
}

#[test]
fn subprocess_failed_displays_program_path_and_status() {
    let e = StorageError::SubprocessFailed {
        program: "mkfs.ext4",
        path: PathBuf::from("/run/m80/vm-1/overlay.ext4"),
        status: "1".into(),
        stderr: "bad magic number".into(),
    };
    let s = format!("{e}");
    assert!(s.contains("mkfs.ext4"), "program must appear: {s}");
    assert!(s.contains("overlay.ext4"), "path must appear: {s}");
    assert!(s.contains('1'), "status must appear: {s}");
    assert!(s.contains("bad magic"), "stderr must appear: {s}");
}

#[test]
fn overlay_template_create_failed_displays_path() {
    let e = StorageError::OverlayTemplateCreateFailed {
        path: PathBuf::from("/run/m80/.rootfs-overlay-template.lock"),
        err: std::io::Error::from(std::io::ErrorKind::TimedOut),
    };
    let s = format!("{e}");
    assert!(
        s.contains("rootfs-overlay-template"),
        "path must appear: {s}"
    );
}

#[test]
fn overlay_template_mismatch_displays_reason() {
    let e = StorageError::OverlayTemplateMismatch {
        path: PathBuf::from("/run/m80/.rootfs-overlay-template.meta"),
        reason: "wrong size".into(),
    };
    let s = format!("{e}");
    assert!(s.contains("wrong size"), "reason must appear: {s}");
}

#[test]
fn overlay_template_clone_failed_displays_paths() {
    let e = StorageError::OverlayTemplateCloneFailed {
        template: PathBuf::from("/run/m80/template.ext4"),
        dest: PathBuf::from("/run/m80/vm-1/rootfs.overlay.ext4"),
        err: std::io::Error::from(std::io::ErrorKind::AlreadyExists),
    };
    let s = format!("{e}");
    assert!(
        s.contains("template.ext4"),
        "template path must appear: {s}"
    );
    assert!(
        s.contains("rootfs.overlay.ext4"),
        "dest path must appear: {s}"
    );
}

#[test]
fn subprocess_failed_e2fsck_displays_exit_and_stderr() {
    let e = StorageError::SubprocessFailed {
        program: "e2fsck",
        path: PathBuf::from("/run/m80/vm-1/scratch.ext4"),
        status: "8".into(),
        stderr: "filesystem corrupted".into(),
    };
    let s = format!("{e}");
    assert!(s.contains("e2fsck"), "expected program name in: {s}");
    assert!(s.contains("8"), "expected exit code in: {s}");
    assert!(
        s.contains("filesystem corrupted"),
        "expected stderr in: {s}"
    );
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
