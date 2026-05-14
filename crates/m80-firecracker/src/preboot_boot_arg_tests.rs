use super::*;

#[test]
fn boot_args_ubuntu_stock() {
    assert_eq!(
        boot_args_for(
            ImageKind::Ubuntu,
            KernelKind::Stock,
            RootfsFormat::Ext4,
            None,
            false,
            &[]
        ),
        "console=ttyS0 reboot=k panic=-1 pci=off init=/m80-guestd m80.workspace=0 m80.rootfs=ext4",
    );
}

#[test]
fn boot_args_ubuntu_stripped() {
    assert_eq!(
        boot_args_for(
            ImageKind::Ubuntu,
            KernelKind::Stripped,
            RootfsFormat::Ext4,
            None,
            false,
            &[]
        ),
        "console=ttyS0 reboot=k panic=-1 pci=off quiet loglevel=0 8250.nr_uarts=1 earlycon=uart8250,io,0x3f8,115200n8 printk.time=1 init=/m80-guestd m80.workspace=0 m80.rootfs=ext4",
    );
}

#[test]
fn boot_args_minimal_stock() {
    assert_eq!(
        boot_args_for(
            ImageKind::Minimal,
            KernelKind::Stock,
            RootfsFormat::Ext4,
            None,
            false,
            &[]
        ),
        "console=ttyS0 reboot=k panic=-1 pci=off init=/m80-guestd m80.workspace=0 m80.rootfs=ext4",
    );
}

#[test]
fn boot_args_minimal_stripped() {
    assert_eq!(
        boot_args_for(
            ImageKind::Minimal,
            KernelKind::Stripped,
            RootfsFormat::Ext4,
            None,
            false,
            &[]
        ),
        "console=ttyS0 reboot=k panic=-1 pci=off quiet loglevel=0 8250.nr_uarts=1 earlycon=uart8250,io,0x3f8,115200n8 printk.time=1 init=/m80-guestd m80.workspace=0 m80.rootfs=ext4",
    );
}

#[test]
fn boot_args_mark_workspace_when_drive_is_present() {
    assert_eq!(
        boot_args_for(
            ImageKind::Minimal,
            KernelKind::Stock,
            RootfsFormat::Ext4,
            None,
            true,
            &[]
        ),
        "console=ttyS0 reboot=k panic=-1 pci=off init=/m80-guestd m80.workspace=1 m80.rootfs=ext4",
    );
}

#[test]
fn boot_args_mark_erofs_rootfs() {
    assert_eq!(
        boot_args_for(
            ImageKind::Minimal,
            KernelKind::Stock,
            RootfsFormat::Erofs,
            None,
            false,
            &[]
        ),
        "console=ttyS0 reboot=k panic=-1 pci=off init=/m80-guestd m80.workspace=0 m80.rootfs=erofs rootfstype=erofs",
    );
}

#[test]
fn boot_args_append_pid_one_network_tokens_after_workspace_marker() {
    assert_eq!(
        boot_args_for(
            ImageKind::Minimal,
            KernelKind::Stock,
            RootfsFormat::Ext4,
            None,
            false,
            &[
                "m80.net=outbound".to_owned(),
                "m80.net.iface=eth0".to_owned(),
            ],
        ),
        "console=ttyS0 reboot=k panic=-1 pci=off init=/m80-guestd m80.workspace=0 m80.rootfs=ext4 m80.net=outbound m80.net.iface=eth0",
    );
}

#[test]
fn boot_args_override_wins_over_kind_default() {
    let custom = "console=ttyS0 my=custom args";
    assert_eq!(
        boot_args_for(
            ImageKind::Minimal,
            KernelKind::Stripped,
            RootfsFormat::Ext4,
            Some(custom),
            false,
            &[],
        ),
        "console=ttyS0 my=custom args m80.workspace=0 m80.rootfs=ext4",
        "explicit override must take precedence regardless of kind and kernel_kind"
    );
    assert_eq!(
        boot_args_for(
            ImageKind::Ubuntu,
            KernelKind::Stock,
            RootfsFormat::Ext4,
            Some(custom),
            true,
            &[]
        ),
        "console=ttyS0 my=custom args m80.workspace=1 m80.rootfs=ext4",
    );
}
