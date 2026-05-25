# PVH Boot

## Contract

m80 does not expose a PVH toggle in `BootSourceConfig`. Firecracker `v1.15.1`
uses the same `/boot-source` request shape for Linux boot and PVH direct boot:
`kernel_image_path`, optional `initrd_path`, and optional `boot_args`.

On x86_64, Firecracker auto-selects PVH direct boot when the supplied vmlinux
ELF contains the Xen `XEN_ELFNOTE_PHYS32_ENTRY` note. Linux emits that note
when built with `CONFIG_PVH=y`.

## m80 Shape

The stripped m80 kernel keeps the existing legacy virtio-mmio device discovery
shape:

- `CONFIG_ACPI` remains disabled.
- `CONFIG_PCI` remains disabled.
- `CONFIG_VIRTIO_MMIO_CMDLINE_DEVICES=y` remains the device discovery path.
- m80 still sends the same `/boot-source` JSON and the same kernel command
  line family.

PVH changes the entry path only. It does not add a Firecracker API field and
does not move m80 to ACPI or PCI transport.

## Evidence

Current stock release kernel:

```text
$ readelf -n /opt/m80/versions/v0.2.20/artifacts/vmlinux
Xen                  0x00000008 Unknown note type: (0x00000012)
description data: d0 04 00 01 00 00 00 00
```

The locally built stripped kernels that predate this bead do not contain the
Xen `0x12` note, which means Firecracker cannot select PVH for them.

PVH-enabled stripped rebuild:

```text
$ cargo run --locked -p m80-image-build -- kernel build --workspace /tank/projects/m80
vmlinux: /tank/projects/m80/crates/m80-image-build/kernels/vmlinux-m80-f5b946847df00e263974e4e6de1ce9b3b45a02680284a9d04913846e6f127ea4.bin

$ readelf -n crates/m80-image-build/kernels/vmlinux-m80-f5b946847df00e263974e4e6de1ce9b3b45a02680284a9d04913846e6f127ea4.bin
Xen                  0x00000008 Unknown note type: (0x00000012)
description data: b0 06 00 01 00 00 00 00
```

Artifact sha256:

```text
8c2783520fe3802c26305636201044c3e3105458d7fef3d4c0358a0127772c62  crates/m80-image-build/kernels/vmlinux-m80-f5b946847df00e263974e4e6de1ce9b3b45a02680284a9d04913846e6f127ea4.bin
```

Real-KVM minimal/idle N=30 comparison against the previous stripped kernel:

| kernel | PVH ELF note | wallclock P50 | `phase_12b_ready_accept` P50 | success |
| --- | --- | ---: | ---: | ---: |
| `vmlinux-m80-613988fdb6a6aaa0f806ec27e6f6e66875e2f768b28a2c0244aa4af3d8e2ac19.bin` | absent | 1632 ms | 942.973 ms | 30/30 |
| `vmlinux-m80-f5b946847df00e263974e4e6de1ce9b3b45a02680284a9d04913846e6f127ea4.bin` | present | 1630 ms | 944.467 ms | 30/30 |

Bench artifacts:

```text
1fb464752098da35e9d12a6659ccf7c6e1860d642930ede968378ab242d48d5d  /tank/tmp/m80-2ggw-pvh-before-n30/snapshots/2026-05-25T02:12:23+00:00.json
d61b1d9bb59f166ebe9ad335f7fa278a732e1b8334ec13d41b705b100b72e7e5  /tank/tmp/m80-2ggw-pvh-after-n30/snapshots/2026-05-25T02:13:25+00:00.json
```

Interpretation: PVH is correct to keep because it aligns the stripped kernel
with Firecracker's supported x86 direct-boot path, but it is not a current
latency optimization for m80. The ready phase moved by +1.494 ms P50 in this
run, far below the bead's 50 ms enablement threshold and in the wrong
direction.

## Regression Coverage

- `crates/m80-image-build/tests/kernel_config_completeness.rs::firecracker_policy_required_legacy_mmio_symbols_are_builtin`
  asserts `CONFIG_PVH=y` as part of the Firecracker policy keep-list.
- `crates/m80-image-build/tests/kernel_build_pipeline.rs::stripped_config_enables_pvh_direct_boot_note`
  pins the stripped config against accidental PVH removal.

## Decision

Keep `CONFIG_PVH=y` in the stripped kernel config. Do not add
`KernelFormat`, `pvh_boot`, or equivalent request fields to
`m80-firecracker-client`; that would document a Firecracker API field that does
not exist in `v1.15.1`. Do not add a runtime stripped-kernel PVH toggle because
the rebuilt kernel already carries the note and the measured latency delta does
not justify another public knob.
