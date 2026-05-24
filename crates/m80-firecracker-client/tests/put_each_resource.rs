//! Each PUT resource round-trips a config through the fixture server and
//! asserts (a) the request JSON shape and (b) `Ok(())` on a 204 response.

mod fixture_server;
use fixture_server::{resp_204, FixtureServer};

use m80_firecracker_client::{
    BootSourceConfig, CacheType, Client, CpuTemplate, DriveConfig, IoEngine, MachineConfig,
    PartialDriveConfig, PmemConfig, VsockConfig,
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
    assert!(
        !result.request.contains("\"initrd_path\""),
        "None fields must be omitted"
    );
}

#[test]
fn put_machine_config_sends_correct_json() {
    let server = FixtureServer::spawn(resp_204()).unwrap();
    let client = Client::new(&server.socket_path).unwrap();
    client
        .put_machine_config(&MachineConfig {
            vcpu_count: 2,
            mem_size_mib: 512,
            smt: false,
            cpu_template: Some(CpuTemplate::T2),
            track_dirty_pages: Some(true),
        })
        .unwrap();
    let result = server.join();
    assert!(result
        .request
        .starts_with("PUT /machine-config HTTP/1.1\r\n"));
    assert!(result.request.contains("\"vcpu_count\":2"));
    assert!(result.request.contains("\"mem_size_mib\":512"));
    assert!(result.request.contains("\"cpu_template\":\"T2\""));
    assert!(result.request.contains("\"track_dirty_pages\":true"));
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
            io_engine: Some(IoEngine::Async),
            cache_type: Some(CacheType::Unsafe),
        })
        .unwrap();
    let result = server.join();
    assert!(
        result
            .request
            .starts_with("PUT /drives/rootfs HTTP/1.1\r\n"),
        "drive_id must appear in URL: {}",
        result.request.lines().next().unwrap()
    );
    assert!(result.request.contains("\"drive_id\":\"rootfs\""));
    assert!(result.request.contains("\"is_root_device\":true"));
    assert!(result.request.contains("\"io_engine\":\"Async\""));
    assert!(result.request.contains("\"cache_type\":\"Unsafe\""));
}

#[test]
fn put_drive_omits_optional_drive_fields_when_unspecified() {
    let server = FixtureServer::spawn(resp_204()).unwrap();
    let client = Client::new(&server.socket_path).unwrap();
    client
        .put_drive(&DriveConfig {
            drive_id: "rootfs".to_owned(),
            path_on_host: PathBuf::from("/var/fc/rootfs.ext4"),
            is_root_device: true,
            is_read_only: true,
            io_engine: None,
            cache_type: None,
        })
        .unwrap();
    let result = server.join();
    assert!(
        !result.request.contains("\"io_engine\""),
        "None io_engine must be omitted"
    );
    assert!(
        !result.request.contains("\"cache_type\""),
        "None cache_type must be omitted"
    );
}

#[test]
fn pmem_config_serializes_exact_firecracker_shape() {
    let config = PmemConfig {
        id: "pmem0".to_owned(),
        path_on_host: PathBuf::from("/var/fc/toolchain.erofs"),
        root_device: false,
        read_only: true,
    };

    let json = serde_json::to_string(&config).expect("serialize");

    assert_eq!(
        json,
        r#"{"id":"pmem0","path_on_host":"/var/fc/toolchain.erofs","root_device":false,"read_only":true}"#
    );
    let parsed: PmemConfig =
        serde_json::from_str(r#"{"id":"pmem1","path_on_host":"/var/fc/layer.erofs"}"#)
            .expect("defaults deserialize");
    assert!(!parsed.root_device);
    assert!(!parsed.read_only);
}

#[test]
fn put_pmem_sends_correct_json_and_url() {
    let server = FixtureServer::spawn(resp_204()).unwrap();
    let client = Client::new(&server.socket_path).unwrap();
    client
        .put_pmem(&PmemConfig {
            id: "pmem0".to_owned(),
            path_on_host: PathBuf::from("/var/fc/toolchain.erofs"),
            root_device: false,
            read_only: true,
        })
        .unwrap();
    let result = server.join();
    assert!(
        result.request.starts_with("PUT /pmem/pmem0 HTTP/1.1\r\n"),
        "id must appear in URL: {}",
        result.request.lines().next().unwrap()
    );
    assert!(result.request.contains("\"id\":\"pmem0\""));
    assert!(result
        .request
        .contains("\"path_on_host\":\"/var/fc/toolchain.erofs\""));
    assert!(result.request.contains("\"root_device\":false"));
    assert!(result.request.contains("\"read_only\":true"));
}

#[test]
fn io_engine_uses_firecracker_pascal_case() {
    let json = serde_json::to_string(&IoEngine::Async).unwrap();
    assert_eq!(json, "\"Async\"");
    let parsed: IoEngine = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, IoEngine::Async);
}

#[test]
fn cache_type_uses_firecracker_pascal_case() {
    let json = serde_json::to_string(&CacheType::Unsafe).unwrap();
    assert_eq!(json, "\"Unsafe\"");
    let parsed: CacheType = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, CacheType::Unsafe);
}

#[test]
fn patch_drive_sends_partial_drive_json_and_url() {
    let server = FixtureServer::spawn(resp_204()).unwrap();
    let client = Client::new(&server.socket_path).unwrap();
    client
        .patch_drive(&PartialDriveConfig {
            drive_id: "workspace_slot_0".to_owned(),
            path_on_host: Some(PathBuf::from("/var/fc/workspace.ext4")),
        })
        .unwrap();
    let result = server.join();
    assert!(
        result
            .request
            .starts_with("PATCH /drives/workspace_slot_0 HTTP/1.1\r\n"),
        "drive_id must appear in PATCH URL: {}",
        result.request.lines().next().unwrap()
    );
    assert!(result.request.contains("\"drive_id\":\"workspace_slot_0\""));
    assert!(result.request.contains("\"path_on_host\""));
    assert!(
        !result.request.contains("\"is_root_device\""),
        "PartialDrive must not carry preboot-only Drive fields"
    );
    assert!(
        !result.request.contains("\"is_read_only\""),
        "PartialDrive must not carry preboot-only Drive fields"
    );
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

#[test]
fn put_entropy_device_sends_empty_config_json() {
    let server = FixtureServer::spawn(resp_204()).unwrap();
    let client = Client::new(&server.socket_path).unwrap();
    client.put_entropy_device().unwrap();
    let result = server.join();
    assert!(result.request.starts_with("PUT /entropy HTTP/1.1\r\n"));
    assert!(result.request.ends_with("\r\n\r\n{}"));
}
