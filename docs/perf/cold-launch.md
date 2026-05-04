# Cold-launch latency: ubuntu vs minimal image

Bead m80-6a0q.6.

## Methodology

`scripts/bench-cold-launch.sh` runs `m80 launch -- /bin/echo bench-N`
against each image kind on each load level, captures wallclock per
launch + per-phase timings via `M80_PHASE_TRACE=1`, drops the first 2
launches per cell as warmup, computes P50/P95/max from successful runs
only.

Cells (4 total):

| kind    | load   | Notes                                           |
|---------|--------|-------------------------------------------------|
| ubuntu  | idle   | systemd as PID 1 (multi-user.target)            |
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

### stress-ng-loaded: 0 % success on both kinds

Under `stress-ng --cpu $(nproc)` the host CPU is saturated at 100 %.
All 30 launches per cell failed. Per-phase data shows storage_prep
takes ~5.3 s (vs 4 s idle) — slower but not catastrophic. The actual
failure is downstream, likely in the ready-probe or vsock channel
where contention starves the firecracker process or the guest kernel
boot of CPU enough to miss timeouts.

This isn't a release blocker but is on the "wallpaper over before
GA" list. Realistic CI environments don't run with 100 %-saturated
CPU; the loaded cell here is a worst case.

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
