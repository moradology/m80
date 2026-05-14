use std::fs;
use std::os::unix::fs::MetadataExt;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use tempfile::TempDir;

use super::install_pid_one_artifacts;

fn install_fixture() -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    let daemon = dir.path().join("source-m80-guestd");
    fs::write(&daemon, b"guestd").unwrap();
    install_pid_one_artifacts(dir.path(), &daemon).unwrap();
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

#[test]
fn unsquashfs_command_disables_xattr_extraction() {
    let command = super::unsquashfs_command(Path::new("source.squashfs"), Path::new("out"));
    let args = command
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect::<Vec<_>>();

    assert_eq!(args, vec!["-no-xattrs", "-d", "out", "source.squashfs"]);
}

#[test]
fn strip_suid_sgid_bits_clears_tree_without_following_symlinks() {
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("bin");
    let nested = dir.path().join("nested");
    let nested_bin = nested.join("helper");
    fs::create_dir(&nested).unwrap();
    fs::write(&bin, b"bin").unwrap();
    fs::write(&nested_bin, b"nested").unwrap();
    std::os::unix::fs::symlink(&bin, dir.path().join("bin-link")).unwrap();
    fs::set_permissions(&bin, fs::Permissions::from_mode(0o6755)).unwrap();
    fs::set_permissions(&nested_bin, fs::Permissions::from_mode(0o2755)).unwrap();

    super::strip_suid_sgid_bits(dir.path()).unwrap();

    assert_eq!(fs::symlink_metadata(&bin).unwrap().mode() & 0o7777, 0o0755);
    assert_eq!(
        fs::symlink_metadata(&nested_bin).unwrap().mode() & 0o7777,
        0o0755
    );
    assert!(fs::symlink_metadata(dir.path().join("bin-link"))
        .unwrap()
        .file_type()
        .is_symlink());
}
