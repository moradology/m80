# Kernel Config SMP And Debug Sections

The stripped m80 kernel is built for the first-line VM shape: one vCPU and
1024 MiB memory. The seed config now disables SMP with
`# CONFIG_SMP is not set` so AP bring-up, IPI, and scheduler paths that cannot
be used by a one-vCPU Firecracker guest are not linked into the stripped
kernel.

The kernel-builder script also runs `strip --strip-all vmlinux` before copying
the artifact to `/out/vmlinux-m80-<config-sha>.bin`. Firecracker loads the ELF
program image directly and does not need kernel symbol tables on the boot path.

Verification checklist for this change:

- `cargo test -p m80-image-build --test kernel_build_pipeline`
- `m80-image-build kernel build --workspace <repo>`
- `file <new-vmlinux>` reports a stripped image
- the new kernel artifact is at most 90% of the previous stripped-kernel size
- real-KVM cold launch N=50 minimal/idle boots successfully and records the
  `phase_12b_ready_accept` delta against the prior stripped-kernel baseline

## 2026-05-13 Measurement

Built artifact:

`crates/m80-image-build/kernels/vmlinux-m80-74dfa25b4022ed4ef3e82d316f259f4fbe822640407217ac1d3b894d1f9903e9.bin`

Artifact verification:

- `file` reports: `ELF 64-bit LSB executable, x86-64, version 1 (SYSV),
  statically linked, BuildID[sha1]=fcead3108733acb99376abc9ee84ed6cdc2c379f,
  stripped`.
- `readelf -S` shows 32 section headers and no `.symtab`, `.strtab`,
  `.rela.*`, or `.debug_*` sections.
- Size changed from 28,330,344 bytes for the prior May 5 stripped-kernel
  reference artifact to 15,810,168 bytes for this build: 55.8% of prior size.
- A temp minimal artifact directory with this kernel and an updated manifest
  passed `m80-image-build verify --rootfs
  /tmp/m80-build/minimal-jp6ik6/output.ext4`.

Real-KVM smoke:

`M80_IMAGE_KIND=minimal IMAGE_BUILD_DIR=/tmp/m80-build/minimal-jp6ik6
M80_KERNEL_KIND=stripped
M80_STRIPPED_KERNEL_PATH=/tmp/m80-build/minimal-jp6ik6/vmlinux
./scripts/smoke.sh launch-only` passed.

Real-KVM bench:

`N=50 WARMUP=2 KIND=minimal SKIP_LOADED=1 KERNEL_KIND=stripped
IMAGE_BUILD_DIR_MINIMAL=/tmp/m80-build/minimal-jp6ik6
M80_BIN=./target/release/m80 ./scripts/bench-cold-launch.sh`

Snapshot:
`crates/m80-firecracker/benches/snapshots/2026-05-13T17:06:51+00:00.json`.

Against the prior matching-source snapshot
`crates/m80-firecracker/benches/snapshots/2026-05-13T16:34:08+00:00.json`:

| metric | prior | new | delta |
|---|---:|---:|---:|
| wallclock P50 | 1728 ms | 1693 ms | -35 ms |
| `phase_12b_ready_accept` P50 | 1,022,520 us | 1,041,442 us | +18,922 us |
| `phase_12b_host_waiting_accept` P50 | 1,020,625 us | 1,039,487 us | +18,862 us |
| `phase_12b_kernel_console_range` P50 | 952,277 us | 931,800 us | -20,477 us |
| `phase_12a_instance_start` P50 | 18,439 us | 12,717 us | -5,722 us |
| guest ready signal P50 | 63,455 us | 60,440 us | -3,015 us |

Interpretation: the size target and boot-correctness gate passed, but the
ready-accept latency target did not. The guest's own milestones moved slightly
faster and the kernel console range shortened by about 20 ms, while host
waiting/ready accept did not improve in this run. Treat this as an artifact
size win plus a small wallclock win, not as proof of the targeted 50 ms
`phase_12b_ready_accept` improvement.
