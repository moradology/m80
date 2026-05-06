use std::path::Path;

use m80_firecracker::{
    firecracker_api_socket_path, rootfs_overlay_path, run_dir_path, scratch_image_path,
    vsock_socket_path,
};

#[test]
fn per_vm_dir_under_run_root() {
    let run_root = Path::new("/run/m80");

    let run_dir = run_dir_path(run_root, "vm-alpha");

    assert_eq!(run_dir, Path::new("/run/m80/vm-alpha"));
}

#[test]
fn api_socket_path_is_inside_jailer_root() {
    let run_dir = Path::new("/run/m80/vm-alpha");
    let firecracker_bin = Path::new("/opt/m80/bin/firecracker");

    let socket = firecracker_api_socket_path(run_dir, firecracker_bin);

    assert_eq!(
        socket,
        Path::new("/run/m80/vm-alpha/firecracker/vm-alpha/root/firecracker.sock")
    );
}

#[test]
fn vsock_socket_path_is_inside_jailer_root() {
    let run_dir = Path::new("/run/m80/vm-alpha");
    let firecracker_bin = Path::new("/opt/m80/bin/firecracker");

    let socket = vsock_socket_path(run_dir, firecracker_bin);

    assert_eq!(
        socket,
        Path::new("/run/m80/vm-alpha/firecracker/vm-alpha/root/vsock.sock")
    );
}

#[test]
fn rootfs_and_scratch_paths_are_colocated() {
    let run_dir = Path::new("/run/m80/vm-alpha");

    assert_eq!(
        rootfs_overlay_path(run_dir),
        Path::new("/run/m80/vm-alpha/rootfs.overlay.ext4")
    );
    assert_eq!(
        scratch_image_path(run_dir),
        Path::new("/run/m80/vm-alpha/scratch.ext4")
    );
}
