use std::path::{Path, PathBuf};

use m80_jailer::jail_root_path;

#[test]
fn jail_root_under_run_dir() {
    let run_dir = Path::new("/var/run/m80/vm-1");
    let firecracker = Path::new("/usr/bin/firecracker");

    assert_eq!(
        jail_root_path(run_dir, firecracker),
        PathBuf::from("/var/run/m80/vm-1/firecracker/vm-1/root")
    );
}

#[test]
fn chroot_base_is_caller_configured_run_dir() {
    let run_dir = Path::new("/tmp/custom-run-root/vm-2");
    let firecracker = Path::new("/opt/m80/fc");

    assert_eq!(
        jail_root_path(run_dir, firecracker),
        PathBuf::from("/tmp/custom-run-root/vm-2/fc/vm-2/root")
    );
}
