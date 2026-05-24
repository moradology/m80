use super::*;
use m80_firecracker_client::{CacheType, CpuTemplate, IoEngine, PmemConfig};

#[path = "preboot_boot_arg_tests.rs"]
mod boot_arg_tests;

const VALID_PMEM_DIGEST: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn plan_without_workspace() -> Vec<PrebootPut> {
    plan_preboot_puts(
        &SandboxConfig::default(),
        "vm-alpha",
        ImageKind::Ubuntu,
        KernelKind::Stock,
        RootfsFormat::Ext4,
        false,
        &[],
        &[],
        &RealizedNetwork::NoEgress,
        &[],
    )
    .unwrap()
}

fn pmem_layer(name: &str) -> crate::PmemLayer {
    let digest = crate::ImageDigest::parse(VALID_PMEM_DIGEST).expect("digest");
    let image = crate::ErofsImageRef::from_digest(digest);
    let mount_at =
        crate::GuestMountPath::parse(&format!("/opt/m80-layers/{name}")).expect("mount path");
    crate::PmemLayer::new(image, crate::PmemSharing::PerVm, mount_at)
}

fn resolved_pmem_backing(slot: usize) -> crate::types::ResolvedPmemBacking {
    crate::types::ResolvedPmemBacking {
        host_path: PathBuf::from(format!("/run/m80/vm/pmem/{slot}.img")),
        jail_basename: format!("pmem.{slot}.img"),
        sharing: crate::PmemSharing::PerVm,
    }
}

#[test]
fn machine_config_put_before_boot() {
    let config = SandboxConfig {
        vcpu_count: Some(2),
        mem_size_mib: Some(2048),
        cpuset_cpus: None,
        cpu_template: None,
        fc_log_level: None,
        ..SandboxConfig::default()
    };

    let puts = plan_preboot_puts(
        &config,
        "vm-alpha",
        ImageKind::Ubuntu,
        KernelKind::Stock,
        RootfsFormat::Ext4,
        false,
        &[],
        &[],
        &RealizedNetwork::NoEgress,
        &[],
    )
    .unwrap();

    let PrebootPut::MachineConfig(machine) = &puts[0] else {
        panic!("first preboot PUT must be machine config");
    };
    assert_eq!(machine.vcpu_count, 2);
    assert_eq!(machine.mem_size_mib, 2048);
    assert!(!machine.smt);
    assert_eq!(machine.cpu_template, None);
    assert_eq!(machine.track_dirty_pages, None);
}

#[test]
fn layer_1_machine_config_omits_optional_fields_by_default() {
    let puts = plan_without_workspace();

    let PrebootPut::MachineConfig(machine) = &puts[0] else {
        panic!("first preboot PUT must be machine config");
    };
    assert_eq!(machine.cpu_template, None);
    assert_eq!(machine.track_dirty_pages, None);
    assert!(!machine.smt);
}

#[test]
fn default_machine_config_serialization_omits_optional_fields() {
    let machine = machine_config_for(&SandboxConfig::default());
    let json = serde_json::to_value(&machine).expect("machine config serializes");

    assert_eq!(json.get("cpu_template"), None);
    assert_eq!(json.get("track_dirty_pages"), None);
}

#[test]
fn machine_config_honors_explicit_cpu_template() {
    let config = SandboxConfig {
        cpu_template: Some(CpuTemplate::T2),
        ..SandboxConfig::default()
    };

    let machine = machine_config_for(&config);
    let json = serde_json::to_value(&machine).expect("machine config serializes");

    assert_eq!(machine.cpu_template, Some(CpuTemplate::T2));
    assert_eq!(
        json.get("cpu_template").and_then(|value| value.as_str()),
        Some("T2")
    );
}

#[test]
fn boot_source_put_before_instance_start() {
    let puts = plan_without_workspace();

    let PrebootPut::BootSource(boot) = &puts[1] else {
        panic!("second preboot PUT must be boot source");
    };
    assert_eq!(boot.kernel_image_path, PathBuf::from("/kernel"));
    assert_eq!(
        boot.boot_args.as_deref(),
        Some("console=ttyS0 reboot=k panic=-1 pci=off init=/m80-guestd m80.workspace=0 m80.rootfs=ext4")
    );
    assert!(boot.initrd_path.is_none());
}

#[test]
fn root_drive_put_with_is_root_device() {
    let puts = plan_without_workspace();

    let PrebootPut::Drive(root) = &puts[2] else {
        panic!("third preboot PUT must be rootfs drive");
    };
    assert_eq!(root.drive_id, "rootfs");
    assert_eq!(root.path_on_host, PathBuf::from("/rootfs.ext4"));
    assert!(root.is_root_device);
    assert!(root.is_read_only);
    assert_eq!(root.io_engine, None);
    assert_eq!(root.cache_type, None);
}

#[test]
fn rootfs_overlay_drive_put_after_shared_rootfs() {
    let puts = plan_without_workspace();

    let PrebootPut::Drive(overlay) = &puts[3] else {
        panic!("fourth preboot PUT must be rootfs overlay drive");
    };
    assert_eq!(overlay.drive_id, "rootfs_overlay");
    assert_eq!(overlay.path_on_host, PathBuf::from("/rootfs.overlay.ext4"));
    assert!(!overlay.is_root_device);
    assert!(!overlay.is_read_only);
    assert_eq!(overlay.io_engine, Some(IoEngine::Async));
    assert_eq!(overlay.cache_type, Some(CacheType::Unsafe));
}

#[test]
fn writable_drive_cache_type_override_preserves_writeback() {
    let config = SandboxConfig {
        drive_cache_type: Some(CacheType::Writeback),
        ..SandboxConfig::default()
    };
    let puts = plan_preboot_puts(
        &config,
        "vm-alpha",
        ImageKind::Ubuntu,
        KernelKind::Stock,
        RootfsFormat::Ext4,
        true,
        &[],
        &[],
        &RealizedNetwork::NoEgress,
        &[],
    )
    .unwrap();

    let PrebootPut::Drive(root) = &puts[2] else {
        panic!("third preboot PUT must be rootfs drive");
    };
    let PrebootPut::Drive(overlay) = &puts[3] else {
        panic!("fourth preboot PUT must be rootfs overlay drive");
    };
    let PrebootPut::Drive(workspace) = &puts[4] else {
        panic!("workspace drive must follow overlay drive");
    };

    assert_eq!(root.cache_type, None);
    assert_eq!(overlay.cache_type, Some(CacheType::Writeback));
    assert_eq!(workspace.cache_type, Some(CacheType::Writeback));
}

#[test]
fn scratch_drive_put_with_workspace_id() {
    let puts = plan_preboot_puts(
        &SandboxConfig::default(),
        "vm-alpha",
        ImageKind::Ubuntu,
        KernelKind::Stock,
        RootfsFormat::Ext4,
        true,
        &[],
        &[],
        &RealizedNetwork::NoEgress,
        &[],
    )
    .unwrap();

    let PrebootPut::Drive(workspace) = &puts[4] else {
        panic!("workspace drive must be inserted before vsock");
    };
    assert_eq!(workspace.drive_id, "workspace");
    assert_eq!(workspace.path_on_host, PathBuf::from("/scratch.ext4"));
    assert!(!workspace.is_root_device);
    assert!(!workspace.is_read_only);
    assert_eq!(workspace.io_engine, Some(IoEngine::Async));
    assert_eq!(workspace.cache_type, Some(CacheType::Unsafe));
}

#[test]
fn scratch_drive_omitted_without_workspace() {
    let puts = plan_without_workspace();

    let workspace_drive = puts.iter().any(|put| {
        matches!(
            put,
            PrebootPut::Drive(DriveConfig { drive_id, .. }) if drive_id == "workspace"
        )
    });

    assert!(!workspace_drive);
}

#[test]
fn preallocated_drive_slots_are_after_rootfs_overlay_and_before_vsock() {
    let config = SandboxConfig {
        preallocated_drive_slots: 2,
        ..SandboxConfig::default()
    };

    let puts = plan_preboot_puts(
        &config,
        "vm-alpha",
        ImageKind::Ubuntu,
        KernelKind::Stock,
        RootfsFormat::Ext4,
        false,
        &[],
        &[],
        &RealizedNetwork::NoEgress,
        &[],
    )
    .unwrap();

    let PrebootPut::Drive(slot0) = &puts[4] else {
        panic!("first hotplug slot must follow rootfs overlay");
    };
    let PrebootPut::Drive(slot1) = &puts[5] else {
        panic!("second hotplug slot must follow first hotplug slot");
    };
    assert_eq!(slot0.drive_id, "hotplug_slot_0");
    assert_eq!(slot0.path_on_host, PathBuf::from("/hotplug-slot-0.raw"));
    assert!(!slot0.is_root_device);
    assert!(!slot0.is_read_only);
    assert_eq!(slot0.cache_type, None);
    assert_eq!(slot1.drive_id, "hotplug_slot_1");
    assert_eq!(slot1.path_on_host, PathBuf::from("/hotplug-slot-1.raw"));
    assert_eq!(slot1.cache_type, None);

    let PrebootPut::EntropyDevice = &puts[6] else {
        panic!("entropy device must follow all preboot drive slots");
    };

    let PrebootPut::Vsock(_) = &puts[7] else {
        panic!("vsock must remain after all preboot drive slots");
    };
}

#[test]
fn outbound_nat_network_interface_put_after_drives_and_before_vsock() {
    let puts = plan_preboot_puts(
        &SandboxConfig::default(),
        "vm-alpha",
        ImageKind::Ubuntu,
        KernelKind::Stock,
        RootfsFormat::Ext4,
        false,
        &[],
        &[],
        &RealizedNetwork::OutboundNat {
            tap_name: "tfc123456789abc".to_owned(),
            vmm_netns_path: "/run/netns/m80n123456789abc".into(),
            guest_mac: "02:00:00:00:00:02".to_owned(),
        },
        &[],
    )
    .unwrap();

    let PrebootPut::NetworkInterface(nic) = &puts[4] else {
        panic!("network interface PUT must follow rootfs overlay drives");
    };
    assert_eq!(nic.iface_id, "eth0");
    assert_eq!(nic.host_dev_name, "tfc123456789abc");
    assert_eq!(nic.guest_mac.as_deref(), Some("02:00:00:00:00:02"));

    let PrebootPut::EntropyDevice = &puts[5] else {
        panic!("entropy device PUT must follow network interface PUT");
    };

    let PrebootPut::Vsock(_) = &puts[6] else {
        panic!("vsock must follow network interface PUT");
    };
}

#[test]
fn pmem_puts_follow_hotplug_slots_and_precede_network_interface() {
    let config = SandboxConfig {
        preallocated_drive_slots: 2,
        pmem_layers: vec![pmem_layer("rust"), pmem_layer("node")],
        ..SandboxConfig::default()
    };
    let backings = vec![resolved_pmem_backing(0), resolved_pmem_backing(1)];

    let puts = plan_preboot_puts(
        &config,
        "vm-alpha",
        ImageKind::Ubuntu,
        KernelKind::Stock,
        RootfsFormat::Ext4,
        true,
        &config.pmem_layers,
        &backings,
        &RealizedNetwork::OutboundNat {
            tap_name: "tfc123456789abc".to_owned(),
            vmm_netns_path: "/run/netns/m80n123456789abc".into(),
            guest_mac: "02:00:00:00:00:02".to_owned(),
        },
        &[],
    )
    .unwrap();

    let kinds = puts
        .iter()
        .map(|put| match put {
            PrebootPut::MachineConfig(_) => "machine",
            PrebootPut::BootSource(_) => "boot",
            PrebootPut::Drive(DriveConfig { drive_id, .. }) => drive_id.as_str(),
            PrebootPut::Pmem(PmemConfig { id, .. }) => id.as_str(),
            PrebootPut::NetworkInterface(_) => "network-interface",
            PrebootPut::EntropyDevice => "entropy",
            PrebootPut::Vsock(_) => "vsock",
        })
        .collect::<Vec<_>>();

    assert_eq!(
        kinds,
        [
            "machine",
            "boot",
            "rootfs",
            "rootfs_overlay",
            "workspace",
            "hotplug_slot_0",
            "hotplug_slot_1",
            "pmem_0",
            "pmem_1",
            "network-interface",
            "entropy",
            "vsock",
        ]
    );

    let PrebootPut::Pmem(pmem0) = &puts[7] else {
        panic!("first pmem PUT must follow hotplug slots");
    };
    assert_eq!(pmem0.id, "pmem_0");
    assert_eq!(pmem0.path_on_host, PathBuf::from("/pmem.0.img"));
    assert!(!pmem0.root_device);
    assert!(pmem0.read_only);
    assert_eq!(puts[7].phase_name(), "phase_11_put_pmem_pmem_0");

    let PrebootPut::Pmem(pmem1) = &puts[8] else {
        panic!("second pmem PUT must follow first pmem PUT");
    };
    assert_eq!(pmem1.id, "pmem_1");
    assert_eq!(pmem1.path_on_host, PathBuf::from("/pmem.1.img"));
}

#[test]
fn pmem_puts_are_omitted_when_layers_are_empty() {
    let puts = plan_without_workspace();

    assert!(!puts.iter().any(|put| matches!(put, PrebootPut::Pmem(_))));
}

#[test]
fn pmem_plan_requires_resolved_backings() {
    let config = SandboxConfig {
        pmem_layers: vec![pmem_layer("rust")],
        ..SandboxConfig::default()
    };

    let Err(err) = plan_preboot_puts(
        &config,
        "vm-alpha",
        ImageKind::Ubuntu,
        KernelKind::Stock,
        RootfsFormat::Ext4,
        false,
        &config.pmem_layers,
        &[],
        &RealizedNetwork::NoEgress,
        &[],
    ) else {
        panic!("pmem plan should require resolved backings")
    };

    assert!(
        matches!(err, FcError::InvalidState { .. }),
        "expected InvalidState, got {err:?}"
    );
}

#[test]
fn pmem_plan_does_not_add_kernel_cmdline_pmem_tokens_or_size() {
    let config = SandboxConfig {
        pmem_layers: vec![pmem_layer("rust")],
        ..SandboxConfig::default()
    };
    let backings = vec![resolved_pmem_backing(0)];

    let puts = plan_preboot_puts(
        &config,
        "vm-alpha",
        ImageKind::Ubuntu,
        KernelKind::Stock,
        RootfsFormat::Ext4,
        false,
        &config.pmem_layers,
        &backings,
        &RealizedNetwork::NoEgress,
        &[],
    )
    .unwrap();

    let boot_args = puts
        .iter()
        .find_map(|put| match put {
            PrebootPut::BootSource(config) => config.boot_args.as_deref(),
            _ => None,
        })
        .expect("boot source args");
    assert!(
        !boot_args.contains("pmem"),
        "pmem config must not be threaded through kernel cmdline: {boot_args}"
    );

    let PrebootPut::Pmem(pmem) = &puts[4] else {
        panic!("pmem PUT must follow rootfs drives");
    };
    let json = serde_json::to_value(pmem).expect("pmem config serializes");
    assert_eq!(json.get("size"), None);
    assert_eq!(
        json.get("path_on_host").and_then(|v| v.as_str()),
        Some("/pmem.0.img")
    );
}

#[test]
fn layer_1_preboot_plan_contains_only_documented_devices() {
    let puts = plan_preboot_puts(
        &SandboxConfig {
            preallocated_drive_slots: 1,
            ..SandboxConfig::default()
        },
        "vm-alpha",
        ImageKind::Ubuntu,
        KernelKind::Stock,
        RootfsFormat::Ext4,
        true,
        &[],
        &[],
        &RealizedNetwork::OutboundNat {
            tap_name: "tfc123456789abc".to_owned(),
            vmm_netns_path: "/run/netns/m80n123456789abc".into(),
            guest_mac: "02:00:00:00:00:02".to_owned(),
        },
        &[],
    )
    .unwrap();

    let kinds = puts
        .iter()
        .map(|put| match put {
            PrebootPut::MachineConfig(_) => "machine",
            PrebootPut::BootSource(_) => "boot",
            PrebootPut::Drive(DriveConfig { drive_id, .. }) => drive_id.as_str(),
            PrebootPut::Pmem(PmemConfig { id, .. }) => id.as_str(),
            PrebootPut::NetworkInterface(_) => "network-interface",
            PrebootPut::EntropyDevice => "entropy",
            PrebootPut::Vsock(_) => "vsock",
        })
        .collect::<Vec<_>>();

    assert_eq!(
        kinds,
        [
            "machine",
            "boot",
            "rootfs",
            "rootfs_overlay",
            "workspace",
            "hotplug_slot_0",
            "network-interface",
            "entropy",
            "vsock",
        ]
    );
}

#[test]
fn entropy_device_put_before_vsock_and_start() {
    let puts = plan_without_workspace();

    let PrebootPut::EntropyDevice = &puts[4] else {
        panic!("entropy device PUT must follow rootfs drives");
    };

    let PrebootPut::Vsock(_) = &puts[5] else {
        panic!("vsock must follow entropy device PUT");
    };
}

#[test]
fn vsock_device_put_before_start() {
    let puts = plan_without_workspace();

    let PrebootPut::Vsock(vsock) = puts.last().unwrap() else {
        panic!("last preboot PUT must be vsock");
    };
    assert_eq!(vsock.guest_cid, m80_vsock::cid_for_vm_id("vm-alpha"));
    assert_eq!(vsock.uds_path, PathBuf::from("/vsock.sock"));
}

#[test]
fn preboot_put_phase_names_include_individual_devices() {
    let puts = plan_preboot_puts(
        &SandboxConfig {
            preallocated_drive_slots: 1,
            ..SandboxConfig::default()
        },
        "vm-alpha",
        ImageKind::Ubuntu,
        KernelKind::Stock,
        RootfsFormat::Ext4,
        true,
        &[],
        &[],
        &RealizedNetwork::OutboundNat {
            tap_name: "tfc123456789abc".to_owned(),
            vmm_netns_path: "/run/netns/m80n123456789abc".into(),
            guest_mac: "02:00:00:00:00:02".to_owned(),
        },
        &[],
    )
    .unwrap();

    let names = puts.iter().map(PrebootPut::phase_name).collect::<Vec<_>>();

    assert_eq!(
        names,
        [
            "phase_11_put_machine_config",
            "phase_11_put_boot_source",
            "phase_11_put_drive_rootfs",
            "phase_11_put_drive_rootfs_overlay",
            "phase_11_put_drive_workspace",
            "phase_11_put_drive_hotplug_slot_0",
            "phase_11_put_network_interface_eth0",
            "phase_11_put_entropy",
            "phase_11_put_vsock",
        ]
    );
}

#[test]
fn machine_config_uses_default_sizing_when_omitted() {
    let config = SandboxConfig {
        vcpu_count: None,
        mem_size_mib: None,
        cpuset_cpus: None,
        cpu_template: None,
        fc_log_level: None,
        ..SandboxConfig::default()
    };

    let machine = machine_config_for(&config);

    assert_eq!(machine.vcpu_count, FIRST_LINE_VCPU_COUNT);
    assert_eq!(machine.mem_size_mib, FIRST_LINE_MEM_SIZE_MIB);
    assert!(!machine.smt);
    assert_eq!(machine.cpu_template, None);
    assert_eq!(machine.track_dirty_pages, None);
}

#[test]
fn machine_config_honors_caller_sizing() {
    let config = SandboxConfig {
        vcpu_count: Some(2),
        mem_size_mib: Some(2048),
        cpuset_cpus: None,
        cpu_template: None,
        fc_log_level: None,
        ..SandboxConfig::default()
    };

    let machine = machine_config_for(&config);

    assert_eq!(machine.vcpu_count, 2);
    assert_eq!(machine.mem_size_mib, 2048);
    assert!(!machine.smt);
    assert_eq!(machine.cpu_template, None);
    assert_eq!(machine.track_dirty_pages, None);
}
