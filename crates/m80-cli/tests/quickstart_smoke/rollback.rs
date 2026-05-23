use super::support::*;

#[test]
fn quickstart_rolls_back_previous_profile_when_config_write_fails() {
    let dir = tempfile::tempdir().unwrap();
    let tarball = write_release_tarball(&dir);
    let mut paths = install_paths(&dir, "rollback");
    std::fs::create_dir_all(&paths.profile_dir).unwrap();
    let profile_path = paths.profile_dir.join("default.toml");
    let stale_profile = "kernel_image = \"/old/vmlinux\"\nrootfs_image = \"/old/rootfs.ext4\"\n";
    std::fs::write(&profile_path, stale_profile).unwrap();
    paths.config_path = dir.path().join("rollback-config-dir");
    std::fs::create_dir(&paths.config_path).unwrap();

    let output = m80()
        .args(quickstart_no_run_args(&tarball, &paths))
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert_eq!(
        std::fs::read_to_string(profile_path).unwrap(),
        stale_profile
    );
}

#[test]
fn quickstart_rolls_back_symlinked_profile_target_when_config_write_fails() {
    let dir = tempfile::tempdir().unwrap();
    let tarball = write_release_tarball(&dir);
    let mut paths = install_paths(&dir, "symlink-rollback");
    std::fs::create_dir_all(&paths.profile_dir).unwrap();
    let profile_path = paths.profile_dir.join("default.toml");
    let profile_target = paths.profile_dir.join("default-target.toml");
    let stale_profile = "kernel_image = \"/old/vmlinux\"\nrootfs_image = \"/old/rootfs.ext4\"\n";
    std::fs::write(&profile_target, stale_profile).unwrap();
    std::os::unix::fs::symlink("default-target.toml", &profile_path).unwrap();
    paths.config_path = dir.path().join("symlink-rollback-config-dir");
    std::fs::create_dir(&paths.config_path).unwrap();

    let output = m80()
        .args(quickstart_no_run_args(&tarball, &paths))
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert_eq!(
        std::fs::read_to_string(profile_target).unwrap(),
        stale_profile
    );
    assert_eq!(
        std::fs::read_link(profile_path).unwrap(),
        std::path::PathBuf::from("default-target.toml")
    );
}

#[test]
fn quickstart_rejects_unknown_existing_config_key_before_profile_commit() {
    let dir = tempfile::tempdir().unwrap();
    let tarball = write_release_tarball(&dir);
    let paths = install_paths(&dir, "bad-config");
    std::fs::create_dir_all(&paths.profile_dir).unwrap();
    let profile_path = paths.profile_dir.join("default.toml");
    let stale_profile = "kernel_image = \"/old/vmlinux\"\nrootfs_image = \"/old/rootfs.ext4\"\n";
    std::fs::write(&profile_path, stale_profile).unwrap();
    std::fs::write(&paths.config_path, "unknown_key = true\n").unwrap();

    let output = m80()
        .args(quickstart_no_run_args(&tarball, &paths))
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unknown config key"),
        "quickstart should report the config key that would make the next run fail; stderr={stderr}"
    );
    assert_eq!(
        std::fs::read_to_string(profile_path).unwrap(),
        stale_profile
    );
}
