use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

#[path = "../common/mod.rs"]
mod common;

use common::m80;

#[test]
fn install_cleanup_removes_inactive_version_dir() {
    let temp = tempfile::tempdir().unwrap();
    let install_root = temp.path().join("install-root");
    let active = seed_version(&install_root, "v1.2.4");
    let old = seed_version(&install_root, "v1.2.3");
    symlink(&active, install_root.join("active")).unwrap();

    let output = m80()
        .args([
            "install-cleanup",
            "--install-root",
            install_root.to_str().unwrap(),
            "--release-tag",
            "v1.2.3",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "cleanup failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.contains("removed inactive install version"),
        "{stdout}"
    );
    assert!(stdout.contains("active_pointer_removed=false"), "{stdout}");
    assert!(!old.exists());
    assert!(active.exists());
    assert_eq!(fs::read_link(install_root.join("active")).unwrap(), active);
}

#[test]
fn install_cleanup_refuses_active_version_by_default() {
    let temp = tempfile::tempdir().unwrap();
    let install_root = temp.path().join("install-root");
    let active = seed_version(&install_root, "v1.2.4");
    symlink(&active, install_root.join("active")).unwrap();

    let output = m80()
        .args([
            "install-cleanup",
            "--install-root",
            install_root.to_str().unwrap(),
            "--release-tag",
            "v1.2.4",
        ])
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("active installed version"), "{stderr}");
    assert!(stderr.contains("--remove-active"), "{stderr}");
    assert!(active.exists());
    assert_eq!(fs::read_link(install_root.join("active")).unwrap(), active);
}

#[test]
fn install_cleanup_can_explicitly_remove_active_version() {
    let temp = tempfile::tempdir().unwrap();
    let install_root = temp.path().join("install-root");
    let active = seed_version(&install_root, "v1.2.4");
    symlink(&active, install_root.join("active")).unwrap();

    let output = m80()
        .args([
            "install-cleanup",
            "--install-root",
            install_root.to_str().unwrap(),
            "--release-tag",
            "v1.2.4",
            "--remove-active",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "active cleanup failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.contains("removed active install version"),
        "{stdout}"
    );
    assert!(stdout.contains("active_pointer_removed=true"), "{stdout}");
    assert!(!active.exists());
    assert!(!install_root.join("active").exists());
}

#[test]
fn install_cleanup_removes_former_active_after_rollback() {
    let temp = tempfile::tempdir().unwrap();
    let install_root = temp.path().join("install-root");
    let rollback_target = seed_version(&install_root, "v1.2.3");
    let previous_active = seed_version(&install_root, "v1.2.4");
    symlink(&rollback_target, install_root.join("active")).unwrap();

    let output = m80()
        .args([
            "install-cleanup",
            "--install-root",
            install_root.to_str().unwrap(),
            "--release-tag",
            "v1.2.4",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "post-rollback cleanup failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!previous_active.exists());
    assert!(rollback_target.exists());
    assert_eq!(
        fs::read_link(install_root.join("active")).unwrap(),
        rollback_target
    );
}

#[test]
fn install_cleanup_refuses_malformed_version_dir() {
    let temp = tempfile::tempdir().unwrap();
    let install_root = temp.path().join("install-root");
    let malformed = install_root.join("versions/v1.2.3");
    fs::create_dir_all(&malformed).unwrap();

    let output = m80()
        .args([
            "install-cleanup",
            "--install-root",
            install_root.to_str().unwrap(),
            "--release-tag",
            "v1.2.3",
        ])
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("malformed version directory"), "{stderr}");
    assert!(malformed.exists());
}

#[test]
fn install_cleanup_refuses_symlink_inside_version_shape() {
    let temp = tempfile::tempdir().unwrap();
    let install_root = temp.path().join("install-root");
    let version_dir = seed_version(&install_root, "v1.2.3");
    fs::remove_file(version_dir.join("bundle.json")).unwrap();
    symlink("/tmp/not-m80-bundle.json", version_dir.join("bundle.json")).unwrap();

    let output = m80()
        .args([
            "install-cleanup",
            "--install-root",
            install_root.to_str().unwrap(),
            "--release-tag",
            "v1.2.3",
        ])
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("not a regular file"), "{stderr}");
    assert!(version_dir.exists());
}

#[test]
fn install_cleanup_refuses_symlinked_install_root() {
    let temp = tempfile::tempdir().unwrap();
    let real_root = temp.path().join("real-root");
    let linked_root = temp.path().join("linked-root");
    seed_version(&real_root, "v1.2.3");
    symlink(&real_root, &linked_root).unwrap();

    let output = m80()
        .args([
            "install-cleanup",
            "--install-root",
            linked_root.to_str().unwrap(),
            "--release-tag",
            "v1.2.3",
        ])
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("install root must not pass through a symlink"),
        "{stderr}"
    );
    assert!(real_root.join("versions/v1.2.3").exists());
}

#[test]
fn install_cleanup_rejects_release_tag_path_escape() {
    let temp = tempfile::tempdir().unwrap();
    let install_root = temp.path().join("install-root");

    let output = m80()
        .args([
            "install-cleanup",
            "--install-root",
            install_root.to_str().unwrap(),
            "--release-tag",
            "../v1.2.3",
        ])
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("not a single version directory name"),
        "{stderr}"
    );
}

fn seed_version(install_root: &Path, tag: &str) -> PathBuf {
    let version_dir = install_root.join("versions").join(tag);
    fs::create_dir_all(version_dir.join("bin")).unwrap();
    fs::create_dir_all(version_dir.join("artifacts")).unwrap();
    fs::write(version_dir.join("bundle.json"), b"{}\n").unwrap();
    fs::write(version_dir.join("bin/m80"), b"#!/bin/sh\n").unwrap();
    version_dir
}
