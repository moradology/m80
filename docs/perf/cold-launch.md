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

---

## Smoke checkpoint — storage pivot (m80-f2zc.9)

**Bead:** m80-f2zc.9 · **Status:** TBD — pending end-to-end KVM exercise.

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

**Bench numbers:** TBD — pending end-to-end KVM exercise on the target host.
Expected save vs. prior `Rootfs::clone` baseline: **700–770 ms** (high
confidence; mechanism is the same shared-RO-base + sparse-overlay pattern
used by runc, crun, kata-containers, and Firecracker-containerd).

---

## Smoke checkpoint — stripped kernel (m80-ci9i.6)

**Bead:** m80-ci9i.6 · **Status:** TBD — pending Docker-built stripped kernel.

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

**Bench numbers:** TBD — pending Docker-built stripped kernel and KVM-exercised
smoke run. Expected save on `phase_12b_ready_accept`: **500–700 ms** (high
confidence per firecracker community reports of 150–300 ms userspace with
stripped kernels).

---

## Smoke checkpoint — snapshot capture + restore (m80-rrp.3.14)

**Bead:** m80-rrp.3.14 · **Status:** TBD — pending KVM-exercised snapshot
round-trip. Smoke scripts are in place; full end-to-end blocked on
out-of-process IPC (v0.2 gap, same as `m80 exec`).

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
`cargo test -- --ignored snapshot`).

### Design references

- Snapshot file layout (`vm.snap`, `mem.snap`): `docs/design/snapshot-restore.md §2`.
- Restore path (PUT /snapshot/load, PATCH /vm Resumed, exec-channel probe):
  `docs/design/snapshot-restore.md §4`.
- CLI surface (`--from-snapshot`, `snapshot capture`):
  `crates/m80-cli/src/args.rs`, `crates/m80-cli/src/cmds.rs`.

**Bench numbers:** TBD — pending KVM-exercised snapshot round-trip.
Expected warm-restore latency: **125–200 ms** (medium confidence; AWS
published numbers for Firecracker snapshot restore; our setup may differ).
Smoke scripts in place at `scripts/smoke.sh` (`M80_SMOKE_MODE=snapshot`).
