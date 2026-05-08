//! Pre-boot Firecracker REST PUT planning.

use std::path::PathBuf;

use m80_firecracker_client::{
    BootSourceConfig, Client, CpuTemplate, DriveConfig, MachineConfig, NetworkInterfaceConfig,
    VsockConfig,
};
use m80_image_manifest::{ImageKind, KernelKind};

use crate::error::FcError;
use crate::layout::preallocated_drive_slot_jail_path;
use crate::types::{
    RealizedNetwork, SandboxConfig, FIRST_LINE_MEM_SIZE_MIB, FIRST_LINE_VCPU_COUNT,
};

/// `panic=-1` triggers immediate reboot on kernel panic (vs. `panic=1`'s
/// 1 s wait). For minimal-kind images where m80-guestd is PID 1, the
/// graceful-stop path exits PID 1 → kernel panics → Firecracker exits;
/// the 1 s wait was pure dead time on every launch.
const COMMON_BOOT_ARGS: &str = "console=ttyS0 reboot=k panic=-1 pci=off";

/// Kernel command-line arguments for Stripped kernels.
///
/// Differences from `COMMON_BOOT_ARGS`:
/// - `quiet loglevel=0` added — suppresses per-device init messages on ttyS0
///   while leaving the console open; fatal panics still print (the panic
///   handler bypasses loglevel). Saves ~20-40 ms of serial flush time on boot.
/// - `8250.nr_uarts=1` added — explicit single-UART cap; prevents probe of
///   the four default UARTs on driver init. Locked at `=1` (not `=0`) per
///   CLAUDE.md "diagnostics before hypotheses": preserving console output is
///   worth more than the ~50 ms saving from suppressing it entirely.
const STRIPPED_BOOT_ARGS: &str =
    "console=ttyS0 reboot=k panic=-1 pci=off quiet loglevel=0 8250.nr_uarts=1";

/// One planned Firecracker REST PUT before `InstanceStart`.
pub(crate) enum PrebootPut {
    /// PUT `/machine-config`.
    MachineConfig(MachineConfig),
    /// PUT `/boot-source`.
    BootSource(BootSourceConfig),
    /// PUT `/drives/{drive_id}`.
    Drive(DriveConfig),
    /// PUT `/network-interfaces/{iface_id}`.
    NetworkInterface(NetworkInterfaceConfig),
    /// PUT `/vsock`.
    Vsock(VsockConfig),
}

/// Build the ordered Firecracker resource PUTs for a cold boot.
pub(crate) fn plan_preboot_puts(
    config: &SandboxConfig,
    vm_id: &str,
    image_kind: ImageKind,
    kernel_kind: KernelKind,
    include_workspace_drive: bool,
    network: &RealizedNetwork,
) -> Vec<PrebootPut> {
    let mut puts = vec![
        PrebootPut::MachineConfig(machine_config_for(config)),
        PrebootPut::BootSource(BootSourceConfig {
            kernel_image_path: PathBuf::from("/kernel"),
            boot_args: Some(boot_args_for(
                image_kind,
                kernel_kind,
                config.boot_args.as_deref(),
                include_workspace_drive,
            )),
            initrd_path: None,
        }),
        PrebootPut::Drive(DriveConfig {
            drive_id: "rootfs".into(),
            path_on_host: PathBuf::from("/rootfs.ext4"),
            is_root_device: true,
            is_read_only: true,
        }),
        PrebootPut::Drive(DriveConfig {
            drive_id: "rootfs_overlay".into(),
            path_on_host: PathBuf::from("/rootfs.overlay.ext4"),
            is_root_device: false,
            is_read_only: false,
        }),
    ];

    if include_workspace_drive {
        puts.push(PrebootPut::Drive(DriveConfig {
            drive_id: "workspace".into(),
            path_on_host: PathBuf::from("/scratch.ext4"),
            is_root_device: false,
            is_read_only: false,
        }));
    }

    for slot in 0..config.preallocated_drive_slots {
        puts.push(PrebootPut::Drive(DriveConfig {
            drive_id: preallocated_drive_slot_id(slot),
            path_on_host: preallocated_drive_slot_jail_path(slot),
            is_root_device: false,
            is_read_only: false,
        }));
    }

    if let RealizedNetwork::OutboundNat {
        tap_name,
        guest_mac,
    } = network
    {
        puts.push(PrebootPut::NetworkInterface(NetworkInterfaceConfig {
            iface_id: "eth0".to_owned(),
            host_dev_name: tap_name.clone(),
            guest_mac: Some(guest_mac.clone()),
        }));
    }

    puts.push(PrebootPut::Vsock(VsockConfig {
        guest_cid: m80_vsock::cid_for_vm_id(vm_id),
        uds_path: PathBuf::from("/vsock.sock"),
    }));

    puts
}

/// Apply the ordered preboot PUT plan to Firecracker.
pub(crate) fn apply_preboot_puts(client: &Client, puts: &[PrebootPut]) -> Result<(), FcError> {
    for put in puts {
        match put {
            PrebootPut::MachineConfig(config) => client.put_machine_config(config)?,
            PrebootPut::BootSource(config) => client.put_boot_source(config)?,
            PrebootPut::Drive(config) => client.put_drive(config)?,
            PrebootPut::NetworkInterface(config) => client.put_network_interface(config)?,
            PrebootPut::Vsock(config) => client.put_vsock(config)?,
        }
    }

    Ok(())
}

pub(crate) fn preallocated_drive_slot_id(slot: u8) -> String {
    format!("hotplug_slot_{slot}")
}

fn machine_config_for(config: &SandboxConfig) -> MachineConfig {
    MachineConfig {
        vcpu_count: config.vcpu_count.unwrap_or(FIRST_LINE_VCPU_COUNT),
        mem_size_mib: config.mem_size_mib.unwrap_or(FIRST_LINE_MEM_SIZE_MIB),
        smt: false,
        cpu_template: Some(CpuTemplate::T2),
    }
}

/// Build kernel boot args for the given `(image_kind, kernel_kind)` pair,
/// honoring any caller override on `SandboxConfig::boot_args`.
fn boot_args_for(
    kind: ImageKind,
    kernel_kind: KernelKind,
    config_override: Option<&str>,
    include_workspace_drive: bool,
) -> String {
    let base = match (config_override, kind, kernel_kind) {
        (Some(custom), _, _) => custom.to_owned(),
        (None, ImageKind::Ubuntu | ImageKind::Minimal, KernelKind::Stock) => {
            format!("{COMMON_BOOT_ARGS} init=/m80-guestd")
        }
        (None, ImageKind::Ubuntu | ImageKind::Minimal, KernelKind::Stripped) => {
            format!("{STRIPPED_BOOT_ARGS} init=/m80-guestd")
        }
    };
    let workspace = u8::from(include_workspace_drive);
    format!("{base} m80.workspace={workspace}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan_without_workspace() -> Vec<PrebootPut> {
        plan_preboot_puts(
            &SandboxConfig::default(),
            "vm-alpha",
            ImageKind::Ubuntu,
            KernelKind::Stock,
            false,
            &RealizedNetwork::NoEgress,
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
        );

        let PrebootPut::MachineConfig(machine) = &puts[0] else {
            panic!("first preboot PUT must be machine config");
        };
        assert_eq!(machine.vcpu_count, 2);
        assert_eq!(machine.mem_size_mib, 2048);
        assert!(!machine.smt);
        assert_eq!(machine.cpu_template, Some(CpuTemplate::T2));
    }

    #[test]
    fn layer_1_machine_config_uses_narrow_cpu_surface() {
        let puts = plan_without_workspace();

        let PrebootPut::MachineConfig(machine) = &puts[0] else {
            panic!("first preboot PUT must be machine config");
        };
        assert_eq!(machine.cpu_template, Some(CpuTemplate::T2));
        assert!(!machine.smt);
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
    fn boot_args_ubuntu_stock() {
        assert_eq!(
            boot_args_for(ImageKind::Ubuntu, KernelKind::Stock, None, false),
            "console=ttyS0 reboot=k panic=-1 pci=off init=/m80-guestd m80.workspace=0",
        );
    }

    #[test]
    fn boot_args_ubuntu_stripped() {
        assert_eq!(
            boot_args_for(ImageKind::Ubuntu, KernelKind::Stripped, None, false),
            "console=ttyS0 reboot=k panic=-1 pci=off quiet loglevel=0 8250.nr_uarts=1 init=/m80-guestd m80.workspace=0",
        );
    }

    #[test]
    fn boot_args_minimal_stock() {
        assert_eq!(
            boot_args_for(ImageKind::Minimal, KernelKind::Stock, None, false),
            "console=ttyS0 reboot=k panic=-1 pci=off init=/m80-guestd m80.workspace=0",
        );
    }

    #[test]
    fn boot_args_minimal_stripped() {
        assert_eq!(
            boot_args_for(ImageKind::Minimal, KernelKind::Stripped, None, false),
            "console=ttyS0 reboot=k panic=-1 pci=off quiet loglevel=0 8250.nr_uarts=1 init=/m80-guestd m80.workspace=0",
        );
    }

    #[test]
    fn boot_args_mark_workspace_when_drive_is_present() {
        assert_eq!(
            boot_args_for(ImageKind::Minimal, KernelKind::Stock, None, true),
            "console=ttyS0 reboot=k panic=-1 pci=off init=/m80-guestd m80.workspace=1",
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
                false
            ),
            "console=ttyS0 my=custom args m80.workspace=0",
            "explicit override must take precedence regardless of kind and kernel_kind"
        );
        assert_eq!(
            boot_args_for(ImageKind::Ubuntu, KernelKind::Stock, Some(custom), true),
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
}
