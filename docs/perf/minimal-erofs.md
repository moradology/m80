# Minimal Erofs Launch Evidence

Bead: `m80-jp6ik.8`

`minimal-erofs` now builds and boots with the stripped kernel, but the measured
launch-latency target was not met.

## Environment

- Host: Linux 6.17.0-22-generic
- Firecracker: v1.15.1
- Kernel: `vmlinux-m80-a096548ede447f98a883c9d0a23b5007f721b2911233186465559f3553992ed0.bin`
- Workload: `m80 run --egress none -- /bin/echo bench-N`
- Samples: `N=50`, `WARMUP=2`, idle host only

## Smoke

Commands:

```bash
M80_IMAGE_KIND=minimal-erofs M80_KERNEL_KIND=stripped \
  M80_STRIPPED_KERNEL_PATH=/tank/projects/m80/crates/m80-image-build/kernels/vmlinux-m80-a096548ede447f98a883c9d0a23b5007f721b2911233186465559f3553992ed0.bin \
  ./scripts/smoke.sh
```

Result: `SMOKE PASSED`, stdout contained `smoke-passes`.

The stock Firecracker kernel cannot boot the erofs rootfs on this host. Guest
console showed:

```text
VFS: Cannot open root device "vda" or unknown-block(254,0): error -19
Kernel panic - not syncing: VFS: Unable to mount root fs on unknown-block(254,0)
```

## Size

| Image | Bytes |
|---|---:|
| `/tmp/m80-build/minimal/output.ext4` | 268435456 |
| `/tmp/m80-build/minimal-erofs/output.erofs` | 3080192 |

The erofs image is about 98.9% smaller than the ext4 equivalent.

## Launch Timing

Warm-cache bench:

| Image | wall P50 | `phase_12b_ready_accept` P50 |
|---|---:|---:|
| minimal ext4 | 1340 ms | 1022994 us |
| minimal erofs | 1340 ms | 1020781 us |

Cold-isolated bench (`--cold-isolation`):

| Image | wall P50 | `phase_12b_ready_accept` P50 | failures |
|---|---:|---:|---:|
| minimal ext4 | 1355 ms | 1031767 us | 0 |
| minimal erofs | 1354 ms | 1022815 us | 2 |

The cold-isolated ready phase improved by 8952 us, below the bead's >=15 ms
acceptance threshold. The warm-cache ready phase improved by 2213 us, and
wall-clock P50 did not move.

