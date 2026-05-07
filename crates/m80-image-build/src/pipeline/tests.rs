use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use tempfile::TempDir;

use super::install_into_rootfs;

fn install_fixture() -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    let daemon = dir.path().join("source-m80-guestd");
    fs::write(&daemon, b"guestd").unwrap();
    install_into_rootfs(dir.path(), &daemon).unwrap();
    dir
}

fn read_link(path: &Path) -> String {
    fs::read_link(path).unwrap().display().to_string()
}

#[test]
fn installs_guest_daemon_binary_at_usr_local_bin() {
    let dir = install_fixture();

    let installed = dir.path().join("usr/local/bin/m80-guestd");
    assert_eq!(fs::read(&installed).unwrap(), b"guestd");
    assert_eq!(
        fs::metadata(&installed).unwrap().mode() & 0o777,
        0o755,
        "guest daemon must be executable"
    );
}

#[test]
fn installs_service_unit_for_basic_target_boot() {
    let dir = install_fixture();

    let unit_path = dir.path().join("etc/systemd/system/m80-guestd.service");
    let unit = fs::read_to_string(unit_path).unwrap();
    assert!(unit.contains("Type=simple"), "{unit}");
    assert!(unit.contains("DefaultDependencies=no"), "{unit}");
    assert!(
        unit.contains("ExecStart=/usr/local/bin/m80-guestd"),
        "{unit}"
    );
    assert!(unit.contains("StandardOutput=journal+console"), "{unit}");
    assert!(unit.contains("StandardError=journal+console"), "{unit}");
    assert!(unit.contains("Restart=on-failure"), "{unit}");
    assert!(unit.contains("WantedBy=basic.target"), "{unit}");
    assert!(!unit.contains("Restart=always"), "{unit}");
    assert!(!unit.contains("EnvironmentFile"), "{unit}");

    assert_eq!(
        read_link(
            &dir.path()
                .join("etc/systemd/system/basic.target.wants/m80-guestd.service")
        ),
        "/etc/systemd/system/m80-guestd.service"
    );
    assert!(
        !dir.path()
            .join("etc/systemd/system/multi-user.target.wants/m80-guestd.service")
            .exists(),
        "m80-guestd must not be gated on multi-user.target"
    );
}

#[test]
fn installs_workspace_mount_unit() {
    let dir = install_fixture();

    let unit = fs::read_to_string(dir.path().join("etc/systemd/system/workspace.mount")).unwrap();
    assert!(unit.contains("What=/dev/vdb"), "{unit}");
    assert!(unit.contains("Where=/workspace"), "{unit}");
    assert!(unit.contains("Type=ext4"), "{unit}");
    assert!(unit.contains("WantedBy=multi-user.target"), "{unit}");
    assert!(dir.path().join("workspace").is_dir());
    assert_eq!(
        read_link(
            &dir.path()
                .join("etc/systemd/system/multi-user.target.wants/workspace.mount")
        ),
        "/etc/systemd/system/workspace.mount"
    );
}

#[test]
fn does_not_install_guestd_environment_file() {
    let dir = install_fixture();

    assert!(
        !dir.path().join("etc/default/m80-guestd").exists(),
        "m80-image-build no longer installs a guestd EnvironmentFile"
    );
    assert!(
        !dir.path().join("etc/default/guestd-rs").exists(),
        "predecessor's guestd-rs EnvironmentFile must not be carried forward"
    );
}
