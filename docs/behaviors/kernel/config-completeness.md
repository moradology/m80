# Kernel Config Completeness

## Scope

This is the Firecracker kernel-policy audit for the committed m80 stripped
kernel config:

- Firecracker source: `docs/kernel-policy.md` from Firecracker tag `v1.15.1`
- m80 config: `crates/m80-image-build/kernel-builder/m80-stripped.config`
- m80 boot mode: x86_64, root block device, Firecracker PCI transport disabled,
  legacy virtio-mmio discovery enabled by kernel command-line devices
- m80 boot args: `pci=off` is part of the base boot args for stock and stripped
  kernels

Firecracker `v1.15.1` supports three x86_64 boot shapes during the ACPI
transition: only ACPI, only legacy mechanisms, or both. m80 deliberately chooses
the only-legacy shape for the stripped kernel because ACPI requires
`CONFIG_PCI=y` even when Firecracker's PCI transport is disabled. That choice is
already captured in `docs/design/stripped-kernel.md` and measured in
`docs/perf/cold-launch.md`.

## Mandatory Built-Ins For m80

These symbols are required by Firecracker policy for m80's selected
legacy-MMIO shape, or by m80's own launch contract on top of that shape. The
regression test at
`crates/m80-image-build/tests/kernel_config_completeness.rs` asserts every row
below is `=y`, not `=m`, absent, or explicitly disabled.

| Symbol | Status | Source and rationale |
| --- | --- | --- |
| `CONFIG_VIRTIO` | present | Parent virtio bus support for Firecracker virtio devices. |
| `CONFIG_VIRTIO_MMIO` | present | Firecracker relevant option for virtio devices. |
| `CONFIG_VIRTIO_MMIO_CMDLINE_DEVICES` | present | x86_64 legacy mechanism; consumes Firecracker's virtio-mmio command-line device descriptions. |
| `CONFIG_VIRTIO_BLK` | present | Root block device boot requirement. |
| `CONFIG_BLK_MQ_VIRTIO` | present | Keeps the virtio block path built in for the selected root-device path. |
| `CONFIG_VIRTIO_NET` | present | Required when m80 enables outbound NAT through eth0. |
| `CONFIG_VIRTIO_VSOCKETS` | present | Required for host/guest command transport. |
| `CONFIG_VSOCKETS` | present | Socket family parent for virtio-vsock. |
| `CONFIG_HW_RANDOM_VIRTIO` | present | Firecracker relevant entropy device; m80 guest crypto paths require it. |
| `CONFIG_RANDOM_TRUST_CPU` | present | Firecracker guest RNG item for supported Linux 5.10+ kernels. |
| `CONFIG_HYPERVISOR_GUEST` | present | KVM guest support dependency group for x86_64. |
| `CONFIG_KVM_GUEST` | present | x86_64 timekeeping and minimal boot requirement. |
| `CONFIG_PVH` | present | Emits the x86 `XEN_ELFNOTE_PHYS32_ENTRY` note; Firecracker `v1.12+` auto-selects PVH direct boot when this note is present. |
| `CONFIG_SERIAL_8250` | present | m80 uses `console=ttyS0` and early serial diagnostics. |
| `CONFIG_SERIAL_8250_CONSOLE` | present | Firecracker boot-log option; m80 depends on serial visibility for debugging. |
| `CONFIG_PRINTK` | present | Firecracker boot-log option; m80 depends on serial diagnostics. |

## Deliberately Absent

These Firecracker policy symbols are not missing by accident. They belong to a
different Firecracker feature or boot shape than the one the stripped m80 kernel
uses.

| Symbol | Status | Rationale |
| --- | --- | --- |
| `CONFIG_ACPI` | deliberately absent | m80 stripped kernels boot through the legacy-MMIO mechanism; ACPI would force PCI support back into the stripped profile. |
| `CONFIG_PCI` | deliberately absent | Firecracker's PCI transport is disabled and m80 passes `pci=off`; PCI is only required for ACPI or Firecracker `--enable-pci`. |
| `CONFIG_BLK_MQ_PCI` | deliberately absent | PCI transport support; not used by the selected legacy-MMIO shape. |
| `CONFIG_PCI_MMCONFIG` | deliberately absent | PCI transport support. |
| `CONFIG_PCI_MSI` | deliberately absent | PCI transport support. |
| `CONFIG_PCIEPORTBUS` | deliberately absent | PCI transport support. |
| `CONFIG_VIRTIO_PCI` | deliberately absent | Firecracker creates MMIO virtio devices when PCI is disabled. |
| `CONFIG_PCI_HOST_COMMON` | deliberately absent | PCI host support, relevant to Firecracker PCI transport and non-x86 notes. |
| `CONFIG_PCI_HOST_GENERIC` | deliberately absent | PCI host support, relevant to Firecracker PCI transport and non-x86 notes. |
| `CONFIG_BLK_DEV_INITRD` | deliberately absent | m80 boots from a root block device, not initrd. |
| `CONFIG_MEMORY_BALLOON` | deliberately absent | m80 does not expose Firecracker balloon devices in the stripped-kernel launch contract. |
| `CONFIG_VIRTIO_BALLOON` | deliberately absent | Same balloon-device exclusion. |
| `CONFIG_MSDOS_PARTITION` | deliberately absent | m80 rootfs artifacts are mounted directly as `/dev/vda`, not by partition UUID. |
| `CONFIG_PTP_1588_CLOCK` | deliberately absent | High-precision KVM PTP clock is not part of the current m80 launch contract. |
| `CONFIG_PTP_1588_CLOCK_KVM` | deliberately absent | Same high-precision clock exclusion. |
| `CONFIG_SERIO_I8042` | deliberately absent | Firecracker clean-shutdown keyboard path is not part of the current stripped-kernel contract. |
| `CONFIG_KEYBOARD_ATKBD` | deliberately absent | Same clean-shutdown keyboard path exclusion. |

## Drift Rule

When m80 changes its Firecracker release pin, this audit must be refreshed
against that exact Firecracker tag. When m80 changes its boot shape to ACPI or
Firecracker PCI transport, the deliberately-absent table stops being valid and
the relevant PCI/ACPI symbols must move into the tested built-in set.
