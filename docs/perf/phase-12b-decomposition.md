# phase_12b decomposition

Date: 2026-05-13

Command:

```sh
N=50 WARMUP=2 KIND=minimal SKIP_LOADED=1 KERNEL_KIND=stripped \
  M80_BIN=./target/release/m80 ./scripts/bench-cold-launch.sh
```

Snapshot:

- `crates/m80-firecracker/benches/snapshots/2026-05-13T15:43:07+00:00.json`

This run used `M80_PHASE_TRACE=1`, which makes stripped-kernel launches append
verbose printk args for measurement. The verbose path is diagnostic-only; normal
stripped launches keep the quiet cmdline.

## Result

| metric | P50 us | P95 us | count |
|---|---:|---:|---:|
| `phase_12b_ready_accept` | 1,028,202 | 1,039,336 | 52 |
| `phase_12b_host_waiting_accept` | 1,026,647 | 1,037,141 | 52 |
| `phase_12b_kernel_console_range` | 952,008 | 960,008 | 52 |
| `phase_12b_guest_process_start` | 2 | 2 | 52 |
| `phase_12b_guest_ready_signal_sent` | 63,451 | 65,982 | 49 |

The phase is dominated by guest kernel boot before `m80-guestd` starts. The
guest userspace path from `process_start` to `ready_signal_sent` is about 63 ms
P50; the host-side ready wait is about 1.03 s P50. The kernel console timestamp
range accounts for about 952 ms P50 of that wait.

## Guest Milestones

| milestone | P50 us |
|---|---:|
| `phase_12b_guest_stdio_redirected` | 1,611 |
| `phase_12b_guest_panic_hook_installed` | 3,306 |
| `phase_12b_guest_pseudo_fs_mounted` | 13,758 |
| `phase_12b_guest_mount_namespace_private` | 20,055 |
| `phase_12b_guest_base_mounted` | 25,277 |
| `phase_12b_guest_overlay_disk_mounted` | 32,499 |
| `phase_12b_guest_overlayfs_mounted` | 43,446 |
| `phase_12b_guest_pivot_rootfs_done` | 51,996 |
| `phase_12b_guest_network_configured` | 55,798 |
| `phase_12b_guest_workspace_absent` | 57,785 |
| `phase_12b_guest_ready_signal_sent` | 63,451 |

No guestd milestone is large enough to explain the 900 ms-class phase. Overlay
and pivot work are visible, but they are tens of milliseconds, not hundreds.

## Phase 11 PUTs

| PUT phase | P50 us | P95 us |
|---|---:|---:|
| `phase_11_put_machine_config` | 208 | 271 |
| `phase_11_put_boot_source` | 90 | 112 |
| `phase_11_put_drive_rootfs` | 96 | 123 |
| `phase_11_put_drive_rootfs_overlay` | 90 | 108 |
| `phase_11_put_vsock` | 212 | 300 |
| `phase_11_rest_puts` | 2,142 | 2,909 |

The individual Firecracker PUTs are not the cold-ready bottleneck.

## Implication

Prioritize kernel-boot work before guestd micro-optimizations. The next
highest-confidence consumers of this data are the kernel cmdline/config beads
(`m80-jp6ik.5`, `.6`, `.23`) and the TLB/cache counter measurement bead
(`m80-jp6ik.46`).
