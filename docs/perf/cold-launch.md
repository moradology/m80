# Cold-launch latency: ubuntu vs minimal image

Bead m80-6a0q.6.

## Methodology

`scripts/bench-cold-launch.sh` runs `m80 run --egress none -- /bin/echo bench-N`
against each image kind on each load level, captures wallclock per
launch + per-phase timings via `M80_PHASE_TRACE=1`, drops the first 2
launches per cell as warmup, computes P50/P95/max from successful runs
only.

Cells (4 total):

| kind    | load   | Notes                                           |
|---------|--------|-------------------------------------------------|
| ubuntu  | idle   | historical schema-v3 run: systemd as PID 1      |
| ubuntu  | loaded | `stress-ng --cpu $(nproc)` running in parallel  |
| minimal | idle   | m80-guestd as PID 1 (no systemd)                |
| minimal | loaded | same stress-ng load                             |

Outputs (long format):
- `crates/m80-firecracker/benches/cold-launch.csv`:
  `timestamp, kind, load, attempt, launch_ms, exit`
- `crates/m80-firecracker/benches/cold-launch-phases.csv`:
  `timestamp, kind, load, attempt, phase, elapsed_us`

## Numbers — N=30 per cell, 48-CPU host, post-graceful-stop

Wallclock (successes only):

| kind / load     | P50 ms | P95 ms | MAX ms | n succeeded | failures |
|-----------------|-------:|-------:|-------:|------------:|---------:|
| ubuntu / idle   |   8922 |   9323 |   9422 |       18/30 |    12/30 |
| ubuntu / loaded |      — |      — |      — |        0/30 |    30/30 |
| minimal / idle  |   3018 |   3218 |   3220 |       30/30 |     0/30 |
| minimal / loaded|      — |      — |      — |        0/30 |    30/30 |

`useful_ms` = total wallclock minus `stop_bounded` (the post-exec
graceful-stop phase). This is the meaningful "launch + exec" budget.

| kind / load     | useful P50 ms |
|-----------------|---------------:|
| ubuntu / idle   |          5692 |
| minimal / idle  |          1696 |

**Speedup: minimal is 3.4× faster than ubuntu for launch+exec on idle.**

## Per-phase attribution (idle, P50 µs)

| phase                       |  ubuntu |  minimal | gap |
|-----------------------------|--------:|---------:|----:|
| phase_1_run_root_prep       |      80 |       70 | ~ |
| phase_2_lease               |      40 |       40 | = |
| phase_3_storage_prep        | 4008429 |   727623 | **5.5× — rootfs clone size (1 GiB vs 256 MiB)** |
| phase_4_jailer_materialize  |     405 |      411 | = |
| phase_5_cgroup_probe        |    2268 |     2293 | = |
| phase_5b_cgroup_create      |   16710 |    16190 | = |
| phase_6_network_realize     |       0 |        0 | (NoEgress) |
| phase_9_jailer_launch       |   25500 |    25492 | = |
| phase_10_open_uds           |      29 |       30 | = |
| phase_11_rest_puts          |     616 |      606 | = |
| phase_12a_instance_start    |   19280 |    18406 | = |
| phase_12b_ready_probe       | 1533499 |   893374 | **1.7× — systemd boot vs no init** |
| exec_send                   |      10 |       11 | = |
| exec_recv                   |   10523 |    10998 | = |
| stop_bounded                | 2006162 |  1061557 | **graceful, 30 s → 1–2 s** |
| stop_release                |       0 |        0 | = |

The two phases that dominate the gap:
- **storage_prep (rootfs clone)**: minimal's 256 MiB rootfs clones in
  ~727 ms; ubuntu's 1 GiB rootfs takes ~4 s. Pure file-copy bandwidth.
- **ready_probe (init system boot)**: minimal's m80-guestd binds vsock
  in ~890 ms (kernel boot + immediate bind); ubuntu's systemd takes
  ~1.5 s reaching multi-user.target.

Other phases are within noise across the two image kinds.

## Graceful-stop migration (path c)

Previously `bounded_stop` issued `SendCtrlAltDel` and waited 30 s for
the guest to honor it. Both common guests ignored it (systemd masks
`ctrl-alt-del.target`; minimal's PID-1 m80-guestd has no signal handler),
so every launch paid a 30 s tax in the stop phase.

Replaced with a `ShutdownRequest`/`ShutdownResponse` envelope on the
existing vsock channel. The guest:
- syncs filesystems
- acks the request with the action it will take (`Exit` for PID-1
  minimal; `Poweroff` for ubuntu's systemd-managed deployment)
- exits (panic-via-exit-1 → kernel reboot for PID 1; `/sbin/poweroff -f`
  for ubuntu)

`stop_bounded` is now **2.0 s ubuntu / 1.1 s minimal** (down from 30 s
both). 30× improvement on the stop phase.

## Known issues

### Ubuntu idle has a 40 % flake rate

12 of 30 ubuntu/idle launches failed (exit 1) with the
`vsock: error adding local-init connection: UnixRead(WouldBlock)` →
`Broken pipe` pattern observed during exec. Successes are tightly
clustered (P50 8922 ms, P95 9323 ms — 1 % spread); failures range
5.9–34 s. Minimal/idle is 30/30 perfect.

Hypothesis: with 10 ms ready-probe cadence (m80-bgas.1) the host fans
many short-lived CONNECT/RST exchanges through Firecracker's vsock
muxer; some interaction with systemd's own boot-time vsock activity is
provoking the muxer's local-init accept-loop into an EAGAIN. The
minimal image, with no systemd, doesn't trip it. Worth a targeted
investigation; not blocking v0.1 since minimal kind is the perf path.

### stress-ng-loaded: 0 % success on both kinds (2026-05-05 baseline)

Under `stress-ng --cpu $(nproc)` the host CPU is saturated at 100 %.
All 30 launches per cell failed. Per-phase data shows storage_prep
takes ~5.3 s (vs 4 s idle) — slower but not catastrophic. The actual
failure is downstream, likely in the ready-probe or vsock channel
where contention starves the firecracker process or the guest kernel
boot of CPU enough to miss timeouts.

This isn't a release blocker but is on the "wallpaper over before
GA" list. Realistic CI environments don't run with 100 %-saturated
CPU; the loaded cell here is a worst case.

### stress-ng-loaded recovery (m80-c78m, 2026-05-08)

The release-gating saturation failure was not a vsock muxer starvation bug.
The visible signal was Firecracker's stderr from the jailer wrapper:
`Failed to exec into Firecracker: Resource temporarily unavailable (os error 11)`.
That error is `EAGAIN` from `exec` after the hardening wrapper applied
`RLIMIT_NPROC`.

`RLIMIT_NPROC` is scoped to the process real UID across the host, not to an
individual microVM. Under same-UID `stress-ng --cpu $(nproc)` load, the default
`nproc = 256` limit could make the wrapper fail before Firecracker started. The
fix is to leave `nproc` unset by default; callers that need a host-wide per-UID
process ceiling can still opt in explicitly.

Current proof after rebuilding `/tmp/m80-build/minimal` with the matching
guestd/protocol version:

| attempt | exit | wallclock ms | stdout | `phase_12b_ready_accept` us |
|---:|---:|---:|---|---:|
| 1 | 0 | 1958 | `c78m-1` | 1074190 |
| 2 | 0 | 1967 | `c78m-2` | 1079927 |
| 3 | 0 | 1807 | `c78m-3` | 1115150 |
| 4 | 0 | 1644 | `c78m-4` | 1051004 |
| 5 | 0 | 1835 | `c78m-5` | 1087164 |

Summary: 5/5 loaded launches succeeded under `stress-ng --cpu $(nproc)`.

## Storage pivot impact (m80-f2zc.7)

Run date: 2026-05-05. Image: freshly rebuilt minimal rootfs at
`/tmp/m80-build/minimal-perf-20260505c`, with the current PID-1
overlay+pivot guestd. Host: Linux 6.17.0-22-generic, Firecracker 1.15.1.

The first real-KVM smoke found two structural blockers before benching:
missing `/lower`/`/upper`/`/merged` mountpoints on the read-only base root,
and a guestd cancel-poll bug where a blocking vsock `fill_buf()` could hold
the exec response until the host timed out. Both were fixed before the
measurements below.

### Minimal idle, N=30

Snapshot: `crates/m80-firecracker/benches/snapshots/2026-05-05T09:02:43+00:00.json`.

| metric | pre-pivot baseline | post-pivot | delta |
|---|---:|---:|---:|
| wallclock P50 | 3018 ms | 1517 ms | -1501 ms |
| useful P50 | 1696 ms | 1207 ms | -489 ms |
| `phase_3_storage_prep` P50 | 727.6 ms | 215.9 ms | -511.7 ms |
| `phase_12b_ready_accept` P50 | 893.4 ms | 906.2 ms | +12.8 ms |
| success rate | 30/30 | 30/30 | unchanged |

Interpretation: the pivot removes the full rootfs copy, but sparse overlay
creation plus `mkfs.ext4` is still about 216 ms p50. Ready latency is
unchanged within noise; the storage pivot did not move kernel boot/guestd
startup.

### Minimal loaded, N=5

Snapshot: `crates/m80-firecracker/benches/snapshots/2026-05-05T09:03:20+00:00.json`.

| metric | post-pivot loaded |
|---|---:|
| success rate | 1/5 |
| successful wallclock P50 | 1644 ms |
| successful useful P50 | 1318 ms |
| `phase_3_storage_prep` P50 across attempts | 254.6 ms |
| `phase_12b_ready_accept` P50 across attempts | 955.0 ms |

The loaded cell is still mostly failing under `stress-ng --cpu 48`. That
supports the original hypothesis that the saturation failure is orthogonal to
storage; the failures are downstream of storage prep.

### 16-VM concurrent probe

One-off command launched 16 minimal VMs concurrently with unique IDs against
the same rebuilt image. Logs are under `/tmp/m80-concurrent-1777971851`.

| metric | result |
|---|---:|
| success rate | 16/16 |
| batch wallclock | 1620 ms |
| concurrent `phase_3_storage_prep` P50 | 271.0 ms |
| concurrent `phase_12b_ready_accept` P50 | 917.1 ms |
| `/proc/meminfo Cached` delta | +4448 KiB |

The small Cached delta is consistent with a shared read-only base image: the
16 launches are not copying or dirtying 16 independent 256 MiB rootfs images.

## Stripped kernel impact (m80-ci9i.4)

Run date: 2026-05-05. Image: same rebuilt minimal rootfs as the storage-pivot
run, with `vmlinux` pointed at
`crates/m80-image-build/kernels/vmlinux-m80-74dfa25b4022ed4ef3e82d316f259f4fbe822640407217ac1d3b894d1f9903e9.bin`.

The first Docker-built stripped kernels did not discover `/dev/vda` and
panicked with `VFS: Cannot open root device "vda" or unknown-block(0,0)`.
Firecracker's own kernel policy clarified the mismatch: x86_64 ACPI boot
requires `CONFIG_ACPI=y` plus `CONFIG_PCI=y`, while the non-PCI legacy-MMIO
path requires `CONFIG_VIRTIO_MMIO_CMDLINE_DEVICES=y`. The stripped kernel now
uses the explicit legacy-MMIO path (`CONFIG_ACPI=n`, `CONFIG_PCI=n`,
`CONFIG_VIRTIO_MMIO_CMDLINE_DEVICES=y`, `pci=off` retained).
The next Ubuntu smoke found systemd could not mount API filesystems until the
keep-list also included cgroups, file-handle syscalls, tmpfs ACL/xattr support,
and systemd's event primitives.

### Minimal idle, N=30

Snapshot: `crates/m80-firecracker/benches/snapshots/2026-05-05T10:05:32+00:00.json`.

| metric | stock post-pivot | stripped | delta |
|---|---:|---:|---:|
| wallclock P50 | 1517 ms | 1417 ms | -100 ms |
| useful P50 | 1207 ms | 1167 ms | -40 ms |
| `phase_3_storage_prep` P50 | 215.9 ms | 216.3 ms | +0.4 ms |
| `phase_12b_ready_accept` P50 | 906.2 ms | 866.0 ms | -40.2 ms |
| success rate | 30/30 | 30/30 | unchanged |

Interpretation: this stripped config is boot-correct for the minimal image and
buys about 100 ms wallclock, but it does not deliver the expected 500-700 ms
cold-boot improvement. The dominant ready phase remains near 0.9 s, so further
kernel work should start from boot diagnostics and config profiling rather than
assuming the current strip list is sufficient.

### Minimal loaded, N=5

Snapshot: `crates/m80-firecracker/benches/snapshots/2026-05-05T10:06:03+00:00.json`.

| metric | stripped loaded |
|---|---:|
| success rate | 0/5 |
| successful wallclock P50 | none |
| useful P50 across phase-bearing attempts | 1242 ms |
| `phase_3_storage_prep` P50 across attempts | 300.2 ms |
| `phase_12b_ready_accept` P50 across attempts | 866.6 ms |

The loaded-cell failure shape matches the stock post-pivot run: one success
out of five for stock and zero out of five for stripped under full CPU
saturation. The stripped kernel did not materially change the orthogonal
stress-ng/vsock timing issue.

### Ubuntu idle, N=30

Stock snapshot: `crates/m80-firecracker/benches/snapshots/2026-05-05T10:12:04+00:00.json`.
Stripped snapshot: `crates/m80-firecracker/benches/snapshots/2026-05-05T10:08:58+00:00.json`.

| metric | stock schema-3 | stripped | delta |
|---|---:|---:|---:|
| wallclock P50 | 3919 ms | 3818 ms | -101 ms |
| useful P50 | 2534 ms | 2435 ms | -99 ms |
| `phase_3_storage_prep` P50 | 1335.0 ms | 1337.0 ms | +2.0 ms |
| `phase_12b_ready_accept` P50 | 1108.0 ms | 1006.8 ms | -101.2 ms |
| success rate | 30/30 | 30/30 | unchanged |

Interpretation: the stripped kernel is now boot-correct for both minimal and
Ubuntu images. It buys about 100 ms on both cells, not the expected 500-700 ms.

## Residual cold fresh launch profile (m80-w4vc)

The cold-launch exploration summary is captured in
`docs/behaviors/lifecycle/cold-launch-phase-profile.md`, with raw machine
data in `docs/behaviors/lifecycle/cold-launch-phase-profile.json`.

On the best current minimal cell, stripped kernel plus post-pivot storage, the
remaining cold-start stack is dominated by:

| phase | P50 | recommendation |
|---|---:|---|
| `phase_12b_ready_accept` | 866.0 ms | instrument guest PID-1 boot milestones before choosing a fix |
| `phase_3_storage_prep` | 216.3 ms | proceed with `m80-f2zc.10` overlay template/reflink work |
| jailed host setup plus `InstanceStart` | about 56 ms | no immediate host/API follow-up |

The no-jailer measurement ceiling is bounded by the small jailed-host phases,
not by the guest-ready or storage costs. It is not a product direction unless
future evidence changes the phase stack.

## Guest PID-1 and overlay-template follow-up (m80-1f8.6, m80-f2zc.10)

Follow-up artifacts:

- Guest milestone summary: `docs/behaviors/lifecycle/guest-boot-milestones.md`.
- Raw guest milestone data: `docs/behaviors/lifecycle/guest-boot-milestones.json`.

After guest PID-1 milestone instrumentation and the overlay-template storage
path, the current minimal cold-launch shape is:

| cell | wallclock P50 | `phase_12b_ready_accept` P50 | guest ready from process start P50 | `phase_3_storage_prep` P50 | `phase_3a_manifest_verify` P50 | `phase_3b_rootfs_prepare` P50 |
|---|---:|---:|---:|---:|---:|---:|
| minimal stock idle, N=30 | 1517 ms | 996.8 ms | 49.2 ms | 180.6 ms | 166.6 ms | 13.7 ms |
| minimal stripped idle, N=30 | 1317 ms | 785.2 ms | 54.6 ms | 180.5 ms | 166.8 ms | 13.8 ms |
| minimal stripped loaded, N=5 | no successful launches | 775.7 ms across failed attempts | not meaningful | 206.5 ms | 187.9 ms | 16.1 ms |

This changes the cold-launch recommendation:

- Guestd PID-1 userspace is not a 100 ms target. Individual guest deltas are
  single-digit milliseconds, and the ready signal is emitted about 49-55 ms
  after guestd process start.
- Kernel/early guest boot remains a plausible target because most
  `phase_12b_ready_accept` time occurs before guestd's first milestone.
- The overlay-template work removed per-launch `mkfs.ext4` as a material
  storage cost. `Rootfs::prepare` is now about 14 ms P50, but total
  `phase_3_storage_prep` remains about 180 ms because manifest sha256
  verification costs about 167 ms P50 on every launch. Follow-up:
  `m80-f2zc.11`.

`m80-f2zc.11` moves that manifest sha256 check back to its existing
trust boundary: `m80-preflight`'s `Rootfs + manifest` check. After this change,
phase 3 no longer emits `phase_3a_manifest_verify`; `phase_3_storage_prep`
is storage work only.

Follow-up run date: 2026-05-05. Source snapshots:

- Before: `crates/m80-firecracker/benches/snapshots/2026-05-05T16:44:54+00:00.json`
- After idle: `crates/m80-firecracker/benches/snapshots/2026-05-05T18:41:55+00:00.json`
- After loaded probe: `crates/m80-firecracker/benches/snapshots/2026-05-05T18:42:30+00:00.json`

| metric | before, minimal stripped idle N=30 | after, minimal stripped idle N=30 | delta |
|---|---:|---:|---:|
| wallclock P50 | 1317 ms | 1117 ms | -200 ms |
| `phase_3_storage_prep` P50 | 180.5 ms | 13.4 ms | -167.1 ms |
| `phase_3a_manifest_verify` P50 | 166.8 ms | removed | -166.8 ms |
| `phase_3b_rootfs_prepare` P50 | 13.8 ms | 13.4 ms | -0.4 ms |
| `phase_12b_ready_accept` P50 | 785.2 ms | 785.3 ms | +0.1 ms |

Loaded N=5 after the change still had 0/5 successful launches under
`stress-ng --cpu 48`, so the saturation failure remains orthogonal. The
phase-bearing failed attempts did show the expected storage shape:
`phase_3_storage_prep` 15.6 ms P50, with no `phase_3a_manifest_verify` row.

## Tail-latency baseline (N=200, minimal/idle, 2026-05-13)

First N>30 run produced by the post-m80-ekbk-B0 harness on a 48-CPU host,
after the MS_BIND remount fix in `m80-jailer/src/plan.rs` and the sudo
escape fix in `bench-cold-launch.sh` (commit `1d7c521`). Captures
extended percentiles + per-phase bootstrap-stable values that the prior
N=30 numbers could not surface.

Wallclock, minimal/idle, N=200, 0 failures, 2 outliers (>2σ):

| metric | value |
|---|---|
| P50 | 1728 ms |
| P75 | 1730 ms |
| P90 | 1732 ms |
| P95 | 1733 ms |
| P99 | 1736 ms |
| P99.9 | 1736 ms |
| max | 1744 ms |
| P50 95% CI | [1728, 1729] ms |

**The tail is tight: P99 − P50 = 8 ms (0.5%).** This is the steady-state
shape after page caches and TLB are warm. Cold-cold numbers (with
`--cold-isolation`) would shift the entire distribution right; the
shape is unmeasured.

Phase tail breakdown (P50 → P99 → max, µs):

| phase | P50 | P99 | max | notes |
|---|---:|---:|---:|---|
| `phase_12b_ready_accept` | 1,016,758 | 1,037,121 | 1,047,369 | **59% of total**; kernel boot + guestd-ready bound by VM cold start |
| `stop_bounded` | 76,634 | 86,898 | 96,366 | graceful-stop RPC + SIGKILL; subtracted from useful_ms |
| `phase_9_jailer_launch` | 25,417 | 25,612 | 25,851 | tight: firecracker jailer exec is deterministic |
| `phase_12a_instance_start` | 18,688 | 23,129 | 24,314 | InstanceStart REST call |
| `phase_5b_cgroup_create` | 18,650 | 26,732 | 41,758 | wider tail; cgroup write path |
| `exec_recv` | 12,047 | 12,857 | 12,998 | guestd response after exec |
| `phase_3_storage_prep` | 11,727 | 14,325 | 17,155 | overlay clone + mkfs |
| `phase_3b_rootfs_prepare` | 11,719 | 14,317 | 17,145 | (same path; dual-tagged) |
| `phase_4_jailer_materialize` | 733 | 3,355 | 3,930 | bind-mount plan execution |
| `phase_5_cgroup_probe` | 2,041 | 2,506 | 2,845 | one-time check |
| `phase_11_rest_puts` | 1,760 | 2,246 | 3,011 | machine + boot config |
| `phase_1_run_root_prep` | 81 | 92 | 155 | filesystem setup |
| `phase_11b_ready_listener_bind` | 47 | 82 | 98 | vsock host bind |
| `phase_11c_boot_identity_record` | 46 | 73 | 370 | jailed identity write; the 370µs max is the only mildly anomalous tail in this run |
| `phase_10_open_uds` | 34 | 52 | 65 | unix domain socket open |
| `phase_2_lease` | 27 | 48 | 94 | admission permit |
| `phase_6_network_realize` | 6 | 8 | 12 | NoEgress: nothing to do |
| `phase_7_outbound_guest_config` | 0 | 5 | 7 | NoEgress: skipped |
| `stop_release` | 16 | 27 | 29 | drop run-dir |

Guest-side boot milestones (µs since guest start, P50 → P99 → max):

| milestone | P50 | P99 | max |
|---|---:|---:|---:|
| `process_start` | 2 | 2 | 2 |
| `panic_hook_installed` | 3,308 | 3,504 | 3,889 |
| `stdio_redirected` | 1,620 | 1,783 | 1,906 |
| `pseudo_fs_mounted` | 13,799 | 15,082 | 16,159 |
| `mount_namespace_private` | 20,079 | 21,328 | 25,453 |
| `base_mounted` | 25,870 | 27,467 | 33,249 |
| `overlay_disk_mounted` | 33,097 | 35,077 | 40,647 |
| `overlay_dirs_ready` | 38,490 | 41,191 | 45,198 |
| `merged_mountpoint_ready` | 41,345 | 44,333 | 48,527 |
| `overlayfs_mounted` | 44,682 | 48,300 | 52,571 |
| `pseudo_fs_bound_into_merged` | 48,149 | 51,732 | 57,306 |
| `merged_self_bound` | 51,539 | 55,058 | 60,857 |
| `pivot_rootfs_done` | 53,319 | 56,801 | 62,689 |
| `overlay_pivot_complete` | 55,245 | 58,675 | 64,634 |
| `network_configured` | 57,127 | 60,532 | 66,573 |
| `workspace_absent` | 59,141 | 62,552 | 68,707 |
| `pid1_setup_complete` | 60,001 | 63,391 | 69,568 |
| `guestd_starting_log` | 61,655 | 65,008 | 71,248 |
| `exec_listener_bound` | 62,702 | 66,001 | 72,375 |
| `ready_signal_sent` | 64,842 | 68,094 | 74,270 |

Guest-side total: ~65 ms P50 from `process_start` to `ready_signal_sent`. The
~950 ms gap between **guest** `ready_signal_sent` and **host**
`phase_12b_ready_accept` is the kernel boot itself before guestd starts
running — the dominant cold-launch cost and the explicit target of
`m80-av33` (Attack the 785 ms `phase_12b_ready_accept`).

This snapshot is checked in at `crates/m80-firecracker/benches/baseline.json`
and is the reference for `--fail-on-regress` gating going forward.

## How to re-run

```bash
# Pre-build both images:
./scripts/smoke.sh                              # ubuntu side
M80_IMAGE_KIND=minimal ./scripts/smoke.sh       # minimal side

# Then bench (default N=30, idle + loaded × ubuntu + minimal):
./scripts/bench-cold-launch.sh

# Or scope down:
N=10 SKIP_LOADED=1 KIND=minimal ./scripts/bench-cold-launch.sh
```

Set `M80_PHASE_TRACE=1` on any single `m80 launch` invocation to get
the same per-phase events streamed to stderr (the bench script does
this automatically).

## Cross-reference

- Smolvm-comparison context: `smolvm-exploration/03-boot-path-and-readiness.md`
  — smolvm reports ~500 ms cold-boot for their minimal libkrun image.
  m80-minimal/idle at 1.7 s useful is ~3.4× slower than smolvm in
  pure cold-boot, but m80 amortizes a real Firecracker jailer setup
  (~100 ms) and rootfs clone (~727 ms) per launch that smolvm doesn't
  do. Apples to oranges.
- Phase-12b ready probe constants: `crates/m80-firecracker/src/launch.rs`
  (READY_POLL_INTERVAL = 10 ms, READY_TIMEOUT = 60 s after m80-bgas.1).
- Graceful-stop constants: `crates/m80-firecracker/src/lifecycle.rs`
  (GRACEFUL_STOP_TIMEOUT = 2 s, SHUTDOWN_RPC_TIMEOUT = 5 s).

---

## Smoke checkpoint — storage pivot (m80-f2zc.9)

**Bead:** m80-f2zc.9 · **Status:** passed on 2026-05-05 against
`/tmp/m80-build/minimal-perf-20260505c`.

### Acceptance criteria (from `docs/planning/perf-roadmap-extended.md §1.3`)

A single launch with `M80_PHASE_TRACE=1` must pass all four assertions:

1. SHA256 of the base file (`output.ext4`) is unchanged before and after the
   launch — verifies the RO base is never written (risk R7 in the perf
   roadmap).
2. The per-VM overlay file (`rootfs.overlay.ext4`) grew by < 100 KB during a
   `/bin/echo` exec — confirms only the guestd-side overlay metadata was
   written, not a full rootfs copy.
3. Inside the guest, `mount` output shows overlayfs at `/` and the lower-dir
   bind-mount is detached post-pivot — confirms `pivot_root` succeeded and
   the old mount tree was detached.
4. `phase_12b_ready_accept` in the `M80_PHASE_TRACE` output completes in
   ≤ 250 ms. A value > 300 ms blocks the BENCH leaf (`m80-f2zc.7`) and
   triggers a diagnostics-first triage per CLAUDE.md.

### How to run

```bash
./scripts/smoke.sh                          # default (ubuntu), full pipeline
M80_IMAGE_KIND=minimal ./scripts/smoke.sh   # minimal image
```

Both invoke the default smoke mode (cold launch → exec → stop). The
`M80_PHASE_TRACE=1` timing assertions listed above are manual inspection
steps, not automated in the script today.

### Wire-level contract — verified by existing tests

The Wave-2 and Wave-3 unit and integration tests verify the wire-level
contract independently of a live KVM run:

- `m80-storage` prepare: `crates/m80-storage/tests/overlay/prepare.rs`
  — verifies `Rootfs::prepare` creates a sparse overlay ext4, base path
  unchanged, overlay path is a separate file.
- Drive PUT order (vda=base RO, vdb=overlay RW, vdc=workspace RW):
  `crates/m80-firecracker/tests/phase11_drive_order.rs`
  — verifies the `phase_11_rest_puts` sequence matches the contract in
  `docs/design/storage-overlay.md §2`.
- In-guest pivot: `crates/m80-guestd/tests/pivot/pivot_rootfs.rs`
  — verifies the kata-derived `pivot_rootfs` sequence (mount, chdir,
  pivot_root, umount2 recursive) against a tmpfs fixture without KVM.

**Bench numbers:** captured in "Storage pivot impact (m80-f2zc.7)" above.
Measured `phase_3_storage_prep` save is **511.7 ms** p50. The result is below
the original 700-770 ms expectation because sparse overlay creation still pays
`mkfs.ext4` (~216 ms p50), but the full rootfs copy is gone.

---

## Smoke checkpoint — stripped kernel (m80-ci9i.6)

**Bead:** m80-ci9i.6 · **Status:** KVM smoke passed on 2026-05-05 with
`vmlinux-m80-74dfa25b4022ed4ef3e82d316f259f4fbe822640407217ac1d3b894d1f9903e9.bin`.

### Acceptance criteria (from `docs/planning/perf-roadmap-extended.md §2.3`)

A single launch with `M80_KERNEL_KIND=stripped` and `M80_PHASE_TRACE=1`
must pass:

1. Guest reaches userspace and `m80-guestd` binds the vsock listener
   (phase_12b_ready_accept completes) — confirms the stripped kernel boots.
2. `phase_12b_ready_accept` ≤ 400 ms. Values > 500 ms trigger
   diagnostics-first triage before producing N=30 bench noise.
3. Console output reaches the host on `ttyS0` (the `8250.nr_uarts=1`
   cmdline pin is honored; one UART preserved for diagnostics).
4. Exec round-trip succeeds — confirms overlayfs is present in the kernel
   (`CONFIG_OVERLAY_FS=y` and `CONFIG_OVERLAY_FS_XINO_AUTO=y` in the
   keep-list per `docs/design/stripped-kernel.md §2`).

### How to run

```bash
# 1. Build the stripped kernel first (requires Docker):
#    See crates/m80-image-build/kernel-builder/Dockerfile (m80-ci9i.2).

# 2. Run the stripped-kernel smoke:
M80_KERNEL_KIND=stripped ./scripts/smoke.sh

# Or point directly at a pre-built artifact:
M80_KERNEL_KIND=stripped \
M80_STRIPPED_KERNEL_PATH=/path/to/vmlinux-m80-<sha>.bin \
./scripts/smoke.sh
```

When `M80_KERNEL_KIND=stripped` is set and no built artifact is found
under `crates/m80-image-build/kernels/vmlinux-m80-*.bin`, the script
exits 0 with an explanatory message (the bench runner can then know the
kernel needs building):

```
# stripped kernel not yet built; skipping (run m80-image-build kernel build first)
```

### Design references

- Kernel config keep-list (overlayfs, vsock, virtio, devtmpfs, 8250):
  `docs/design/stripped-kernel.md §2`.
- Cmdline trim (`quiet loglevel=0 8250.nr_uarts=1`):
  `docs/design/stripped-kernel.md §6`.
- Risk register (R1–R7): `docs/design/stripped-kernel.md §7`.

**Bench numbers:** captured in "Stripped kernel impact (m80-ci9i.4)" above.
The stripped kernel is boot-correct for both minimal and Ubuntu images. It
improves minimal/idle `phase_12b_ready_accept` from 906.2 ms to 866.0 ms p50
and Ubuntu/idle from 1108.0 ms to 1006.8 ms p50.

---

## Smoke checkpoint — snapshot capture + restore (m80-rrp.3.14)

**Bead:** m80-rrp.3.14 · **Status:** KVM in-process capture + restore
round-trip passed on 2026-05-05 via
`crates/m80-firecracker/tests/snapshot_integration.rs`.

### Acceptance criteria (from `docs/planning/perf-roadmap-extended.md §3.3`)

A single round-trip must pass:

1. `m80 launch` (cold) + exec `/bin/echo hello` succeeds.
2. `m80 snapshot capture <vm-id> --store-root <dir>` writes `vm.snap` and
   `mem.snap` to the directory.
3. `m80 launch --from-snapshot <dir> -- /bin/echo restored` exits 0 and
   stdout contains `"restored"`.
4. `restore useful_ms` < `cold useful_ms` − 500 ms (lower-bound floor; less
   than this triggers diagnostics-first triage before BENCH).
5. SHA256 of the base file unchanged after the round-trip.
6. Overlay disk at restore stays small (fresh `mkfs.ext4`, not stale upper
   dir from capture time).

### How to run

```bash
M80_SMOKE_MODE=snapshot ./scripts/smoke.sh
```

### v0.1 status

`m80 snapshot capture` is a v0.1 stub (exit 7 = `EXIT_NOT_IMPLEMENTED`).
The same out-of-process IPC gap that blocks `m80 exec` blocks capture:
both require a side-channel to a running VM. The CLI parse layer and error
path are exercised by the smoke script (exit 7 is asserted; a panic or
unknown-subcommand failure is a bug).

`m80 launch --from-snapshot` is fully wired. The missing-file error path
(exit 6 = `EXIT_CONFIG`) is exercised by the smoke script when no snapshot
files are present.

The full capture+restore end-to-end is covered by the in-process
integration test at `crates/m80-firecracker/tests/snapshot_integration.rs`
(requires KVM, `#[ignore]` by default; run with
`cargo test -p m80-firecracker --test snapshot_integration -- --ignored
--nocapture --test-threads=1`).

### Design references

- Snapshot file layout (`vm.snap`, `mem.snap`): `docs/design/snapshot-restore.md §2`.
- Restore path (PUT /snapshot/load, PATCH /vm Resumed, exec-channel probe):
  `docs/design/snapshot-restore.md §4`.
- CLI surface (`--from-snapshot`, `snapshot capture`):
  `crates/m80-cli/src/args.rs`, `crates/m80-cli/src/cmds.rs`.

**Bench numbers:** captured in `docs/behaviors/snapshot/restore-latency.md`.
Idle restore-ready N=50 is 274.204 ms p50 / 280.132 ms p95. Loaded restore
N=50 is 444.972 ms p50 / 588.221 ms p95.
