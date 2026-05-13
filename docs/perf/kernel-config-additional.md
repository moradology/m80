# Kernel Config Additional Cuts

This experiment builds on `docs/perf/kernel-config-smp.md`. It evaluates a
second set of explicit Kconfig pins for the stripped kernel:

- `CONFIG_HZ_100=y`, `CONFIG_HZ=100`, and `CONFIG_NO_HZ_IDLE=y`
- `CONFIG_PREEMPT_NONE=y`
- `# CONFIG_BLK_DEV_INITRD is not set` and all `CONFIG_RD_*` decompressors off
- `# CONFIG_SCHED_DEBUG is not set` and `# CONFIG_SCHEDSTATS is not set`
- `# CONFIG_DEBUG_INFO is not set` and `# CONFIG_DEBUG_INFO_BTF is not set`
- `# CONFIG_PRINTK_TIME is not set`

Verification checklist for this change:

- `cargo test -p m80-image-build --test kernel_build_pipeline`
- `m80-image-build kernel build --workspace <repo>`
- `file <new-vmlinux>` reports a stripped image
- the rebuilt kernel validates through a manifest with
  `m80-image-build verify --rootfs <rootfs>`
- real-KVM minimal smoke passes
- real-KVM cold launch N=50 minimal/idle records the delta against the
  `CONFIG_SMP=n` baseline from `docs/perf/kernel-config-smp.md`

The close gate is intentionally stricter than the config-only tests. Per
`docs/perf/measurement-playbook.md#e9-kernel-cmdline-and-config-delta`, any
knob that does not measure positive should be removed or recorded as a blocked
negative result rather than kept as hopeful configuration.

## 2026-05-13 Measurement

Built artifact:

`crates/m80-image-build/kernels/vmlinux-m80-9c80aef636281be89d5038d12b0ca25a49f088c925e6a349a5c58bdcdf2c8c66.bin`

Artifact verification:

- `file` reports: `ELF 64-bit LSB executable, x86-64, version 1 (SYSV),
  statically linked, BuildID[sha1]=e6233bd0063feda5e5379505b0b2b2d01ba1709b,
  stripped`.
- `readelf -S` shows 32 section headers and no `.symtab`, `.strtab`,
  `.rela.*`, or `.debug_*` sections.
- Size changed from 15,810,168 bytes for the `.6` baseline kernel to
  15,818,672 bytes for this build: +8,504 bytes.
- A temp minimal artifact directory with this kernel and an updated manifest
  passed `m80-image-build verify --rootfs
  /tmp/m80-build/minimal-jp6ik23/output.ext4`.

Real-KVM smoke:

`M80_IMAGE_KIND=minimal IMAGE_BUILD_DIR=/tmp/m80-build/minimal-jp6ik23
M80_KERNEL_KIND=stripped
M80_STRIPPED_KERNEL_PATH=/tmp/m80-build/minimal-jp6ik23/vmlinux
./scripts/smoke.sh launch-only` passed.

Real-KVM bench:

`N=50 WARMUP=2 KIND=minimal SKIP_LOADED=1 KERNEL_KIND=stripped
IMAGE_BUILD_DIR_MINIMAL=/tmp/m80-build/minimal-jp6ik23
M80_BIN=./target/release/m80 ./scripts/bench-cold-launch.sh`

Snapshot:
`crates/m80-firecracker/benches/snapshots/2026-05-13T17:13:54+00:00.json`.

Against the `.6` baseline snapshot
`crates/m80-firecracker/benches/snapshots/2026-05-13T17:06:51+00:00.json`:

| metric | `.6` baseline | additional config | delta |
|---|---:|---:|---:|
| wallclock P50 | 1693 ms | 1595 ms | -98 ms |
| wallclock P95 | 1697 ms | 1696 ms | -1 ms |
| `phase_12b_ready_accept` P50 | 1,041,442 us | 1,029,564 us | -11,878 us |
| `phase_12b_host_waiting_accept` P50 | 1,039,487 us | 1,027,837 us | -11,650 us |
| `phase_12b_kernel_console_range` P50 | 931,800 us | 915,953 us | -15,847 us |
| `phase_12a_instance_start` P50 | 12,717 us | 12,247 us | -470 us |
| `phase_5b_cgroup_create` P50 | 20,262 us | 18,738 us | -1,524 us |
| guest ready signal P50 | 60,440 us | 60,586 us | +146 us |

Interpretation: the aggregate setting set is boot-correct and directionally
positive on the gated host wait/ready-accept phase, but it missed the original
30 ms target. The clearest kernel-side movement is the console range, which
shortened by about 15.8 ms. Guest-side milestones are effectively flat, so this
does not prove a guest userspace improvement. Keep the config as a small
measured win, not as the major phase_12b break-through the bead hoped for.
