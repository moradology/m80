//! Pre-boot Firecracker REST PUT planning.

use std::path::PathBuf;
use std::time::Instant;

use m80_firecracker_client::{
    BootSourceConfig, CacheType, Client, DriveConfig, IoEngine, MachineConfig,
    NetworkInterfaceConfig, PmemConfig, VsockConfig,
};
use m80_image_manifest::{ImageKind, KernelKind, RootfsFormat};

use crate::error::{ConfigError, FcError};
use crate::layout::{
    pmem_layer_jail_basename, pmem_layer_jail_path, preallocated_drive_slot_jail_path,
};
use crate::pmem::PmemLayer;
use crate::types::{
    RealizedNetwork, ResolvedPmemBacking, SandboxConfig, FIRST_LINE_MEM_SIZE_MIB,
    FIRST_LINE_VCPU_COUNT,
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
    "console=ttyS0 reboot=k panic=-1 pci=off quiet loglevel=0 8250.nr_uarts=1 earlycon=uart8250,io,0x3f8,115200n8 printk.time=1";

/// One planned Firecracker REST PUT before `InstanceStart`.
pub(crate) enum PrebootPut {
    /// PUT `/machine-config`.
    MachineConfig(MachineConfig),
    /// PUT `/boot-source`.
    BootSource(BootSourceConfig),
    /// PUT `/drives/{drive_id}`.
    Drive(DriveConfig),
    /// PUT `/pmem/{id}`.
    Pmem(PmemConfig),
    /// PUT `/network-interfaces/{iface_id}`.
    NetworkInterface(NetworkInterfaceConfig),
    /// PUT `/entropy`.
    EntropyDevice,
    /// PUT `/vsock`.
    Vsock(VsockConfig),
}

impl PrebootPut {
    fn phase_name(&self) -> String {
        match self {
            PrebootPut::MachineConfig(_) => "phase_11_put_machine_config".to_owned(),
            PrebootPut::BootSource(_) => "phase_11_put_boot_source".to_owned(),
            PrebootPut::Drive(config) => format!("phase_11_put_drive_{}", config.drive_id),
            PrebootPut::Pmem(config) => format!("phase_11_put_pmem_{}", config.id),
            PrebootPut::NetworkInterface(config) => {
                format!("phase_11_put_network_interface_{}", config.iface_id)
            }
            PrebootPut::EntropyDevice => "phase_11_put_entropy".to_owned(),
            PrebootPut::Vsock(_) => "phase_11_put_vsock".to_owned(),
        }
    }
}

/// Build the ordered Firecracker resource PUTs for a cold boot.
#[allow(clippy::too_many_arguments)]
pub(crate) fn plan_preboot_puts(
    config: &SandboxConfig,
    vm_id: &str,
    image_kind: ImageKind,
    kernel_kind: KernelKind,
    rootfs_format: RootfsFormat,
    include_workspace_drive: bool,
    pmem_layers: &[PmemLayer],
    pmem_backings: &[ResolvedPmemBacking],
    network: &RealizedNetwork,
    extra_boot_args: &[String],
) -> Result<Vec<PrebootPut>, FcError> {
    validate_caller_boot_args_if_present(config.boot_args.as_deref())?;
    validate_cmdline_tokens("extra_boot_args", extra_boot_args)?;
    validate_resolved_pmem_backings(pmem_layers, pmem_backings)?;

    let mut puts = vec![
        PrebootPut::MachineConfig(machine_config_for(config)),
        PrebootPut::BootSource(BootSourceConfig {
            kernel_image_path: PathBuf::from("/kernel"),
            boot_args: Some(boot_args_for(
                image_kind,
                kernel_kind,
                rootfs_format,
                config.boot_args.as_deref(),
                include_workspace_drive,
                extra_boot_args,
            )?),
            initrd_path: None,
        }),
        PrebootPut::Drive(DriveConfig {
            drive_id: "rootfs".into(),
            path_on_host: PathBuf::from("/rootfs.ext4"),
            is_root_device: true,
            is_read_only: true,
            io_engine: None,
            cache_type: None,
        }),
        PrebootPut::Drive(DriveConfig {
            drive_id: "rootfs_overlay".into(),
            path_on_host: PathBuf::from("/rootfs.overlay.ext4"),
            is_root_device: false,
            is_read_only: false,
            io_engine: Some(IoEngine::Async),
            cache_type: Some(writable_drive_cache_type(config)),
        }),
    ];

    if include_workspace_drive {
        puts.push(PrebootPut::Drive(DriveConfig {
            drive_id: "workspace".into(),
            path_on_host: PathBuf::from("/scratch.ext4"),
            is_root_device: false,
            is_read_only: false,
            io_engine: Some(IoEngine::Async),
            cache_type: Some(writable_drive_cache_type(config)),
        }));
    }

    for slot in 0..config.preallocated_drive_slots {
        puts.push(PrebootPut::Drive(DriveConfig {
            drive_id: preallocated_drive_slot_id(slot),
            path_on_host: preallocated_drive_slot_jail_path(slot),
            is_root_device: false,
            is_read_only: false,
            io_engine: None,
            cache_type: None,
        }));
    }

    for slot in 0..pmem_layers.len() {
        puts.push(PrebootPut::Pmem(PmemConfig {
            id: format!("pmem_{slot}"),
            path_on_host: pmem_layer_jail_path(slot),
            root_device: false,
            read_only: true,
        }));
    }

    match network {
        RealizedNetwork::OutboundNat {
            tap_name,
            guest_mac,
            ..
        }
        | RealizedNetwork::JoinNetns {
            tap_name,
            guest_mac,
            ..
        } => {
            puts.push(PrebootPut::NetworkInterface(NetworkInterfaceConfig {
                iface_id: "eth0".to_owned(),
                host_dev_name: tap_name.clone(),
                guest_mac: Some(guest_mac.clone()),
            }));
        }
        RealizedNetwork::NoEgress => {}
    }

    puts.push(PrebootPut::EntropyDevice);

    puts.push(PrebootPut::Vsock(VsockConfig {
        guest_cid: m80_vsock::cid_for_vm_id(vm_id),
        uds_path: PathBuf::from("/vsock.sock"),
    }));

    Ok(puts)
}

/// Apply the ordered preboot PUT plan to Firecracker.
pub(crate) fn apply_preboot_puts(
    client: &Client,
    puts: &[PrebootPut],
    vm_id: &str,
) -> Result<(), FcError> {
    for put in puts {
        let phase_name = put.phase_name();
        let started = Instant::now();
        match put {
            PrebootPut::MachineConfig(config) => client.put_machine_config(config)?,
            PrebootPut::BootSource(config) => client.put_boot_source(config)?,
            PrebootPut::Drive(config) => client.put_drive(config)?,
            PrebootPut::Pmem(config) => client.put_pmem(config)?,
            PrebootPut::NetworkInterface(config) => client.put_network_interface(config)?,
            PrebootPut::EntropyDevice => client.put_entropy_device()?,
            PrebootPut::Vsock(config) => client.put_vsock(config)?,
        }
        crate::diagnostics::phase_event(&phase_name, vm_id, started.elapsed());
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
        cpu_template: config.cpu_template,
    }
}

fn writable_drive_cache_type(config: &SandboxConfig) -> CacheType {
    config.drive_cache_type.unwrap_or(CacheType::Unsafe)
}

fn validate_resolved_pmem_backings(
    pmem_layers: &[PmemLayer],
    pmem_backings: &[ResolvedPmemBacking],
) -> Result<(), FcError> {
    if pmem_layers.len() != pmem_backings.len() {
        return Err(FcError::InvalidState {
            expected: "one resolved pmem backing per declared layer",
            actual: "pmem layer/backing count mismatch",
        });
    }

    for (slot, backing) in pmem_backings.iter().enumerate() {
        if backing.jail_basename != pmem_layer_jail_basename(slot) {
            return Err(FcError::InvalidState {
                expected: "slot-indexed pmem jail basename",
                actual: "pmem backing jail basename mismatch",
            });
        }
    }

    Ok(())
}

/// Validate caller-supplied boot args before admission consumes a permit.
pub(crate) fn validate_caller_boot_args_if_present(boot_args: Option<&str>) -> Result<(), FcError> {
    if let Some(boot_args) = boot_args {
        validate_caller_boot_args(boot_args)?;
    }
    Ok(())
}

fn validate_caller_boot_args(boot_args: &str) -> Result<(), FcError> {
    if boot_args.chars().any(|ch| ch.is_control() && ch != ' ') {
        return Err(FcError::Config(ConfigError::InvalidValue {
            field: "boot_args",
            reason: "kernel cmdline extras must be printable space-separated tokens".to_owned(),
        }));
    }

    for token in boot_args.split_whitespace() {
        if caller_token_is_reserved(token) {
            return Err(FcError::Config(ConfigError::InvalidValue {
                field: "boot_args",
                reason: format!("reserved kernel cmdline token {token:?} is owned by m80"),
            }));
        }
    }
    Ok(())
}

fn caller_token_is_reserved(token: &str) -> bool {
    token.starts_with("init=")
        || token.starts_with("m80.workspace=")
        || token.starts_with("m80.rootfs=")
        || token.starts_with("rootfstype=")
}

fn validate_cmdline_tokens(field: &'static str, tokens: &[String]) -> Result<(), FcError> {
    for token in tokens {
        if token.is_empty() || token.chars().any(char::is_whitespace) {
            return Err(FcError::Config(ConfigError::InvalidValue {
                field,
                reason: format!("cmdline token {token:?} is empty or contains whitespace"),
            }));
        }
    }
    Ok(())
}

fn boot_args_for(
    kind: ImageKind,
    kernel_kind: KernelKind,
    rootfs_format: RootfsFormat,
    config_override: Option<&str>,
    include_workspace_drive: bool,
    extra_boot_args: &[String],
) -> Result<String, FcError> {
    validate_caller_boot_args_if_present(config_override)?;
    validate_cmdline_tokens("extra_boot_args", extra_boot_args)?;

    let phase_trace_verbose_kernel = kernel_kind == KernelKind::Stripped
        && std::env::var("M80_PHASE_TRACE").is_ok_and(|value| value == "1");
    let base = match (kind, kernel_kind) {
        (ImageKind::Ubuntu | ImageKind::Minimal, KernelKind::Stock) => {
            format!("{COMMON_BOOT_ARGS} init=/m80-guestd")
        }
        (ImageKind::Ubuntu | ImageKind::Minimal, KernelKind::Stripped) => {
            format!("{STRIPPED_BOOT_ARGS} init=/m80-guestd")
        }
    };
    let workspace = u8::from(include_workspace_drive);
    let mut args = format!("{base} m80.workspace={workspace}");
    match rootfs_format {
        RootfsFormat::Ext4 => args.push_str(" m80.rootfs=ext4"),
        RootfsFormat::Erofs => args.push_str(" m80.rootfs=erofs rootfstype=erofs"),
    }
    if phase_trace_verbose_kernel {
        args.push_str(" ignore_loglevel loglevel=7");
    }
    if !extra_boot_args.is_empty() {
        args.push(' ');
        args.push_str(&extra_boot_args.join(" "));
    }
    if let Some(custom) = config_override {
        if !custom.trim().is_empty() {
            args.push(' ');
            args.push_str(custom.trim());
        }
    }
    Ok(args)
}

#[cfg(test)]
#[path = "preboot_tests.rs"]
mod tests;
