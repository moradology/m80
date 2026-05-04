//! Each PUT resource round-trips a config through the fixture server and
//! asserts (a) the request JSON shape and (b) `Ok(())` on a 204 response.

mod fixture_server;
use fixture_server::{FixtureServer, resp_204};

use m80_firecracker_client::{
    BootSourceConfig, Client, DriveConfig, MachineConfig, VsockConfig,
};
use std::path::PathBuf;

#[test]
fn put_boot_source_sends_correct_json() {
    let server = FixtureServer::spawn(resp_204()).unwrap();
    let client = Client::new(&server.socket_path).unwrap();
    client
        .put_boot_source(&BootSourceConfig {
            kernel_image_path: PathBuf::from("/opt/kernel/vmlinux"),
            boot_args: Some("console=ttyS0 reboot=k".to_owned()),
            initrd_path: None,
        })
        .unwrap();
    let result = server.join();
    assert!(
        result.request.starts_with("PUT /boot-source HTTP/1.1\r\n"),
        "unexpected request line"
    );
    assert!(result.request.contains("\"kernel_image_path\""));
    assert!(result.request.contains("/opt/kernel/vmlinux"));
    assert!(result.request.contains("\"boot_args\""));
    assert!(!result.request.contains("\"initrd_path\""), "None fields must be omitted");
}

#[test]
fn put_machine_config_sends_correct_json() {
    let server = FixtureServer::spawn(resp_204()).unwrap();
    let client = Client::new(&server.socket_path).unwrap();
    client
        .put_machine_config(&MachineConfig { vcpu_count: 2, mem_size_mib: 512, smt: false })
        .unwrap();
    let result = server.join();
    assert!(result.request.starts_with("PUT /machine-config HTTP/1.1\r\n"));
    assert!(result.request.contains("\"vcpu_count\":2"));
    assert!(result.request.contains("\"mem_size_mib\":512"));
}

#[test]
fn put_drive_sends_correct_json_and_url() {
    let server = FixtureServer::spawn(resp_204()).unwrap();
    let client = Client::new(&server.socket_path).unwrap();
    client
        .put_drive(&DriveConfig {
            drive_id: "rootfs".to_owned(),
            path_on_host: PathBuf::from("/var/fc/rootfs.ext4"),
            is_root_device: true,
            is_read_only: false,
        })
        .unwrap();
    let result = server.join();
    assert!(
        result.request.starts_with("PUT /drives/rootfs HTTP/1.1\r\n"),
        "drive_id must appear in URL: {}", result.request.lines().next().unwrap()
    );
    assert!(result.request.contains("\"drive_id\":\"rootfs\""));
    assert!(result.request.contains("\"is_root_device\":true"));
}

#[test]
fn put_vsock_sends_correct_json() {
    let server = FixtureServer::spawn(resp_204()).unwrap();
    let client = Client::new(&server.socket_path).unwrap();
    client
        .put_vsock(&VsockConfig {
            guest_cid: 3,
            uds_path: PathBuf::from("/run/fc/vsock.sock"),
        })
        .unwrap();
    let result = server.join();
    assert!(result.request.starts_with("PUT /vsock HTTP/1.1\r\n"));
    assert!(result.request.contains("\"guest_cid\":3"));
    assert!(result.request.contains("/run/fc/vsock.sock"));
}
