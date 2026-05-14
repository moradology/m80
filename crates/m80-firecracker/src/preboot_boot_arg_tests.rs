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
        )
        .unwrap(),
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
        )
        .unwrap(),
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
        )
        .unwrap(),
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
        )
        .unwrap(),
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
        )
        .unwrap(),
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
        )
        .unwrap(),
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
        )
        .unwrap(),
        "console=ttyS0 reboot=k panic=-1 pci=off init=/m80-guestd m80.workspace=0 m80.rootfs=ext4 m80.net=outbound m80.net.iface=eth0",
    );
}

#[test]
fn boot_args_caller_tokens_append_after_m80_tokens() {
    assert_eq!(
        boot_args_for(
            ImageKind::Minimal,
            KernelKind::Stripped,
            RootfsFormat::Ext4,
            Some("m80.malicious_attack=noop"),
            false,
            &[],
        )
        .unwrap(),
        "console=ttyS0 reboot=k panic=-1 pci=off quiet loglevel=0 8250.nr_uarts=1 earlycon=uart8250,io,0x3f8,115200n8 printk.time=1 init=/m80-guestd m80.workspace=0 m80.rootfs=ext4 m80.malicious_attack=noop",
    );
}

#[test]
fn boot_args_reject_init_override() {
    let err = boot_args_for(
        ImageKind::Minimal,
        KernelKind::Stock,
        RootfsFormat::Ext4,
        Some("init=/bin/sh"),
        false,
        &[],
    )
    .unwrap_err();
    assert!(
        matches!(
            err,
            FcError::Config(ConfigError::InvalidValue {
                field: "boot_args",
                ..
            })
        ),
        "expected boot_args ConfigError, got {err:?}"
    );
}

#[test]
fn boot_args_reject_generated_token_whitespace() {
    let err = boot_args_for(
        ImageKind::Minimal,
        KernelKind::Stock,
        RootfsFormat::Ext4,
        None,
        false,
        &["m80.net.mac=02:00:00:00:80:01 init=/bin/sh".to_owned()],
    )
    .unwrap_err();
    assert!(
        matches!(
            err,
            FcError::Config(ConfigError::InvalidValue {
                field: "extra_boot_args",
                ..
            })
        ),
        "expected extra_boot_args ConfigError, got {err:?}"
    );
}
