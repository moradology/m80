# Guest Boot Milestones

Bead: `m80-1f8.6`.

Raw artifact: `docs/behaviors/lifecycle/guest-boot-milestones.json`.

## Method

`m80-guestd` emits structured guest milestone rows to stderr:

```text
M80_GUEST_BOOT name=<milestone> elapsed_us=<micros> delta_us=<micros>
```

When `M80_PHASE_TRACE=1`, `m80-cli` forwards those rows from the guest
console log before tearing the VM down. `scripts/bench-cold-launch.sh` records
both `guest_elapsed_<name>` and `guest_delta_<name>` phase rows, so the same
bench summary includes host phases and guest PID-1 milestones.

## Pmem Mount Row

Pmem layer mounts happen after guestd has already sent the ready signal, so
they are not emitted as PID-1 `M80_GUEST_BOOT` rows. Bench and smoke readers
should treat the host phase below as the boot-path milestone for pmem-enabled
cold launches:

| milestone | source row | meaning |
|---|---|---|
| `pmem_layers_mounted` | `M80_PHASE name=phase_13_pmem_guest_mount` | All declared pmem layers were mounted as guest erofs+DAX layers before the first caller workload. |

Source snapshots:

| cell | snapshot |
|---|---|
| minimal stock idle, N=30 | `crates/m80-firecracker/benches/snapshots/2026-05-05T16:43:32+00:00.json` |
| minimal stripped idle, N=30 | `crates/m80-firecracker/benches/snapshots/2026-05-05T16:44:54+00:00.json` |
| minimal stripped loaded, N=5 | `crates/m80-firecracker/benches/snapshots/2026-05-05T16:45:29+00:00.json` |

## Headline

Guestd PID-1 userspace is not the next 100 ms cold-launch target. On the
current minimal images, the guest ready signal is sent about 49-55 ms after
`m80-guestd` process start. The large `phase_12b_ready_accept` residual is
therefore before or around guestd process start: Firecracker/kernel/early
guest boot, not guestd's overlay pivot code.

## Idle Results

| metric | minimal stock | minimal stripped |
|---|---:|---:|
| success | 30/30 | 30/30 |
| wallclock P50 | 1517 ms | 1317 ms |
| `phase_12b_ready_accept` P50 | 996.8 ms | 785.2 ms |
| `guest_elapsed_pid1_setup_complete` P50 | 44.4 ms | 49.3 ms |
| `guest_elapsed_ready_signal_sent` P50 | 49.2 ms | 54.6 ms |
| largest guest delta P50 | 6.6 ms (`overlay_disk_mounted`) | 5.7 ms (`pseudo_fs_mounted`) |

Selected stripped-kernel guest timeline:

| milestone | elapsed P50 | delta P50 |
|---|---:|---:|
| `pseudo_fs_mounted` | 8.1 ms | 5.7 ms |
| `base_mounted` | 16.0 ms | 3.7 ms |
| `overlay_disk_mounted` | 20.1 ms | 4.2 ms |
| `overlay_dirs_ready` | 23.8 ms | 4.3 ms |
| `overlayfs_mounted` | 31.4 ms | 3.9 ms |
| `pivot_rootfs_done` | 41.9 ms | 2.8 ms |
| `workspace_absent` | 48.1 ms | 3.2 ms |
| `pid1_setup_complete` | 49.3 ms | 1.3 ms |
| `exec_listener_bound` | 52.5 ms | 0.9 ms |
| `ready_signal_sent` | 54.6 ms | 2.1 ms |

## Loaded Evidence

The stripped loaded cell used the existing `stress-ng --cpu $(nproc)` shape and
had 0/5 successful launches. That makes loaded wallclock and guest milestone
distributions non-meaningful for this bead. The phase-bearing failed attempts
still show the same host-side shape: `phase_12b_ready_accept` around 775.7 ms
P50 and `phase_3_storage_prep` around 206.5 ms P50.

## Recommendation

No guestd PID-1 substep can plausibly save 100 ms P50. Keep the milestone
surface because it cheaply prevents future guesswork, but do not create a
ready-before-finish or lazy-mount implementation bead from this data.

The remaining cold fresh launch candidates are:

- Kernel/early-guest boot instrumentation, because most ready-accept time
  elapses before guestd's first milestone.
- Manifest verification placement, because overlay-template cloning made
  `Rootfs::prepare` about 14 ms P50 but `phase_3a_manifest_verify` still costs
  about 167 ms P50 per launch. Follow-up: `m80-f2zc.11`.
