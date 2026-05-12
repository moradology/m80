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
fn installs_guest_daemon_binary_as_pid_one_target() {
    let dir = install_fixture();

    let installed = dir.path().join("m80-guestd");
    assert_eq!(fs::read(&installed).unwrap(), b"guestd");
    assert_eq!(
        fs::metadata(&installed).unwrap().mode() & 0o777,
        0o755,
        "guest daemon must be executable"
    );
}

#[test]
fn installs_init_symlink_for_pid_one_boot() {
    let dir = install_fixture();

    assert_eq!(read_link(&dir.path().join("init")), "/m80-guestd");
}

#[test]
fn installs_pid_one_mountpoint_dirs() {
    let dir = install_fixture();

    for dir_name in super::PID_ONE_MOUNTPOINT_DIRS {
        assert!(
            dir.path().join(dir_name).is_dir(),
            "/{dir_name} must exist for PID-1 overlay setup"
        );
    }
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

#[test]
fn does_not_install_systemd_units_for_guestd_startup() {
    let dir = install_fixture();

    assert!(
        !dir.path()
            .join("etc/systemd/system/m80-guestd.service")
            .exists(),
        "Ubuntu image kind now boots guestd as PID 1, not as a systemd service"
    );
    assert!(
        !dir.path()
            .join("etc/systemd/system/workspace.mount")
            .exists(),
        "workspace is mounted by PID-1 guestd from /dev/vdc, not systemd from /dev/vdb"
    );
}
