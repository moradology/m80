use super::*;

fn plan_without_workspace() -> Vec<PrebootPut> {
    plan_preboot_puts(
        &SandboxConfig::default(),
        "vm-alpha",
        ImageKind::Ubuntu,
        KernelKind::Stock,
        false,
        &RealizedNetwork::NoEgress,
        &[],
    )
}

#[test]
fn machine_config_put_before_boot() {
    let config = SandboxConfig {
        vcpu_count: Some(2),
        mem_size_mib: Some(2048),
        ..SandboxConfig::default()
    };

    let puts = plan_preboot_puts(
        &config,
        "vm-alpha",
        ImageKind::Ubuntu,
        KernelKind::Stock,
        false,
        &RealizedNetwork::NoEgress,
        &[],
    );

    let PrebootPut::MachineConfig(machine) = &puts[0] else {
        panic!("first preboot PUT must be machine config");
    };
    assert_eq!(machine.vcpu_count, 2);
    assert_eq!(machine.mem_size_mib, 2048);
    assert!(!machine.smt);
    assert_eq!(machine.cpu_template, host_cpu_template());
}

#[test]
fn layer_1_machine_config_uses_narrow_cpu_surface() {
    let puts = plan_without_workspace();

    let PrebootPut::MachineConfig(machine) = &puts[0] else {
        panic!("first preboot PUT must be machine config");
    };
    assert_eq!(machine.cpu_template, host_cpu_template());
    assert!(!machine.smt);
}

#[test]
fn cpu_template_uses_t2_only_on_intel_hosts() {
    assert_eq!(
        cpu_template_for_cpuinfo("vendor_id\t: GenuineIntel\n"),
        Some(CpuTemplate::T2)
    );
    assert_eq!(
        cpu_template_for_cpuinfo("vendor_id\t: AuthenticAMD\n"),
        None
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
        Some("console=ttyS0 reboot=k panic=-1 pci=off init=/m80-guestd m80.workspace=0")
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
}

#[test]
fn scratch_drive_put_with_workspace_id() {
    let puts = plan_preboot_puts(
        &SandboxConfig::default(),
        "vm-alpha",
        ImageKind::Ubuntu,
        KernelKind::Stock,
        true,
        &RealizedNetwork::NoEgress,
        &[],
    );

    let PrebootPut::Drive(workspace) = &puts[4] else {
        panic!("workspace drive must be inserted before vsock");
    };
    assert_eq!(workspace.drive_id, "workspace");
    assert_eq!(workspace.path_on_host, PathBuf::from("/scratch.ext4"));
    assert!(!workspace.is_root_device);
    assert!(!workspace.is_read_only);
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
        false,
        &RealizedNetwork::NoEgress,
        &[],
    );

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
    assert_eq!(slot1.drive_id, "hotplug_slot_1");
    assert_eq!(slot1.path_on_host, PathBuf::from("/hotplug-slot-1.raw"));

    let PrebootPut::Vsock(_) = &puts[6] else {
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
        false,
        &RealizedNetwork::OutboundNat {
            tap_name: "tfc123456789abc".to_owned(),
            guest_mac: "02:00:00:00:00:02".to_owned(),
        },
        &[],
    );

    let PrebootPut::NetworkInterface(nic) = &puts[4] else {
        panic!("network interface PUT must follow rootfs overlay drives");
    };
    assert_eq!(nic.iface_id, "eth0");
    assert_eq!(nic.host_dev_name, "tfc123456789abc");
    assert_eq!(nic.guest_mac.as_deref(), Some("02:00:00:00:00:02"));

    let PrebootPut::Vsock(_) = &puts[5] else {
        panic!("vsock must follow network interface PUT");
    };
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
        true,
        &RealizedNetwork::OutboundNat {
            tap_name: "tfc123456789abc".to_owned(),
            guest_mac: "02:00:00:00:00:02".to_owned(),
        },
        &[],
    );

    let kinds = puts
        .iter()
        .map(|put| match put {
            PrebootPut::MachineConfig(_) => "machine",
            PrebootPut::BootSource(_) => "boot",
            PrebootPut::Drive(DriveConfig { drive_id, .. }) => drive_id.as_str(),
            PrebootPut::NetworkInterface(_) => "network-interface",
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
            "vsock",
        ]
    );
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
        true,
        &RealizedNetwork::OutboundNat {
            tap_name: "tfc123456789abc".to_owned(),
            guest_mac: "02:00:00:00:00:02".to_owned(),
        },
        &[],
    );

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
            "phase_11_put_vsock",
        ]
    );
}

#[test]
fn boot_args_ubuntu_stock() {
    assert_eq!(
        boot_args_for(ImageKind::Ubuntu, KernelKind::Stock, None, false, &[]),
        "console=ttyS0 reboot=k panic=-1 pci=off init=/m80-guestd m80.workspace=0",
    );
}

#[test]
fn boot_args_ubuntu_stripped() {
    assert_eq!(
        boot_args_for(ImageKind::Ubuntu, KernelKind::Stripped, None, false, &[]),
        "console=ttyS0 reboot=k panic=-1 pci=off quiet loglevel=0 8250.nr_uarts=1 earlycon=uart8250,io,0x3f8,115200n8 printk.time=1 init=/m80-guestd m80.workspace=0",
    );
}

#[test]
fn boot_args_minimal_stock() {
    assert_eq!(
        boot_args_for(ImageKind::Minimal, KernelKind::Stock, None, false, &[]),
        "console=ttyS0 reboot=k panic=-1 pci=off init=/m80-guestd m80.workspace=0",
    );
}

#[test]
fn boot_args_minimal_stripped() {
    assert_eq!(
        boot_args_for(ImageKind::Minimal, KernelKind::Stripped, None, false, &[]),
        "console=ttyS0 reboot=k panic=-1 pci=off quiet loglevel=0 8250.nr_uarts=1 earlycon=uart8250,io,0x3f8,115200n8 printk.time=1 init=/m80-guestd m80.workspace=0",
    );
}

#[test]
fn boot_args_mark_workspace_when_drive_is_present() {
    assert_eq!(
        boot_args_for(ImageKind::Minimal, KernelKind::Stock, None, true, &[]),
        "console=ttyS0 reboot=k panic=-1 pci=off init=/m80-guestd m80.workspace=1",
    );
}

#[test]
fn boot_args_append_pid_one_network_tokens_after_workspace_marker() {
    assert_eq!(
        boot_args_for(
            ImageKind::Minimal,
            KernelKind::Stock,
            None,
            false,
            &[
                "m80.net=outbound".to_owned(),
                "m80.net.iface=eth0".to_owned(),
            ],
        ),
        "console=ttyS0 reboot=k panic=-1 pci=off init=/m80-guestd m80.workspace=0 m80.net=outbound m80.net.iface=eth0",
    );
}

#[test]
fn boot_args_override_wins_over_kind_default() {
    let custom = "console=ttyS0 my=custom args";
    assert_eq!(
        boot_args_for(
            ImageKind::Minimal,
            KernelKind::Stripped,
            Some(custom),
            false,
            &[],
        ),
        "console=ttyS0 my=custom args m80.workspace=0",
        "explicit override must take precedence regardless of kind and kernel_kind"
    );
    assert_eq!(
        boot_args_for(
            ImageKind::Ubuntu,
            KernelKind::Stock,
            Some(custom),
            true,
            &[]
        ),
        "console=ttyS0 my=custom args m80.workspace=1",
    );
}

#[test]
fn machine_config_uses_default_sizing_when_omitted() {
    let config = SandboxConfig {
        vcpu_count: None,
        mem_size_mib: None,
        ..SandboxConfig::default()
    };

    let machine = machine_config_for(&config);

    assert_eq!(machine.vcpu_count, FIRST_LINE_VCPU_COUNT);
    assert_eq!(machine.mem_size_mib, FIRST_LINE_MEM_SIZE_MIB);
    assert!(!machine.smt);
}

#[test]
fn machine_config_honors_caller_sizing() {
    let config = SandboxConfig {
        vcpu_count: Some(2),
        mem_size_mib: Some(2048),
        ..SandboxConfig::default()
    };

    let machine = machine_config_for(&config);

    assert_eq!(machine.vcpu_count, 2);
    assert_eq!(machine.mem_size_mib, 2048);
    assert!(!machine.smt);
}
