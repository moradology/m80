//! Regression coverage for rootfs proc-fd pinning.

use std::fs;

use m80_preflight::PinnedRootfs;

#[test]
fn proc_fd_path_reads_original_rootfs_after_path_replacement() {
    let dir = tempfile::tempdir().unwrap();
    let rootfs_path = dir.path().join("rootfs.ext4");
    fs::write(&rootfs_path, b"verified rootfs").unwrap();
    let rootfs_file = fs::File::open(&rootfs_path).unwrap();
    let pinned = PinnedRootfs::from_file(rootfs_path.clone(), rootfs_file);

    let replacement = dir.path().join("replacement-rootfs.ext4");
    fs::write(&replacement, b"replacement rootfs").unwrap();
    fs::rename(&replacement, &rootfs_path).unwrap();

    assert_eq!(fs::read(pinned.proc_fd_path()).unwrap(), b"verified rootfs");
    assert_eq!(pinned.path(), rootfs_path.as_path());
}
