# Perf measurement playbook

This playbook is the index for the measurement-track work under
`m80-jp6ik`. Each experiment below states when to run it, the exact harness
entry point, where the result lands, and how later gated beads cite the
measurement.

Measurement-shaped beads use the scaffolded-vs-verified discipline from
ADR 0002. A harness, parser, or mock run is scaffolded. A bead is verified only
when the committed artifact came from the real substrate named by that
experiment: KVM host, `/dev/kvm`, `sudo`, Firecracker/jailer binaries, and real
m80 images.

## Shared Rules

- Run before/after comparisons on the same host in the same session.
- Use `N >= 20` per cell for directional claims and `N >= 50` for tail-latency
  or close-gate claims.
- Commit the raw artifact and the interpretation doc. Do not close from console
  output alone.
- Record host context in the interpretation doc: kernel, Firecracker version,
  image path or artifact hash, CPU count, CPU driver/governor when relevant, and
  whether page cache was dropped.
- If a run uses temporary instrumentation, keep the instrumentation commit,
  artifact commit, and cleanup commit easy to audit.
- Do not rely on implementation-specific `grep` behavior in reproduction
  instructions. Prefer structured outputs, `rg`, Python parsing, or simple POSIX
  shell constructs.
- Close a measured bead with `verified: <artifact-path> @ <commit-sha>`.

## E1. Density Extended

Gates: `m80-jp6ik.13`, `m80-jp6ik.20`.

Purpose: find the first concurrency wall above the currently characterized
C=8 envelope, then attribute whether the wall is no-egress host setup, outbound
network setup, cgroup setup, or a non-m80 host limit.

Invocation:

```sh
N=20 WARMUP=2 KIND=minimal KERNEL_KIND=stripped \
  ./scripts/bench-density-extended.sh
```

The wrapper runs no-egress C=1,2,4,8,16,32,48,64 and outbound
C=1,2,4,8,16,32 through `bench-cold-launch.sh` with `CONCURRENT` and `EGRESS`.
C=64 is an oversubscription probe on the 48-CPU bench host, not a production
target.

Artifacts:

- `crates/m80-firecracker/benches/concurrent.csv`
- `crates/m80-firecracker/benches/snapshots/density-extended.json`
- `docs/perf/density-extended.md`

Required interpretation: table of concurrency by network mode with
wall-time-to-all-ready P50/P95/P99, per-VM P99, success rate, and a named
first-wall point.

## E2. Cold-Cold Restore Baseline

Gates: `m80-jp6ik.34`.

Purpose: establish restore-path cold-cache tax before adding page-cache priming
for snapshot files.

Invocation:

```sh
N=50 ./scripts/bench-restore-cold.sh --cold-isolation
```

If the restore harness lands inside `scripts/bench-cold-launch.sh`, use the
equivalent restore-mode flag and record the exact command in
`docs/perf/restore-latency.md`.

Artifacts:

- `crates/m80-firecracker/benches/snapshots/cold-restore-N50.json`
- `docs/perf/restore-latency.md`

Required interpretation: cold-cache and warm-cache restore P50/P95/P99, plus
restore-phase decomposition that identifies `mem.snap`, `vm.snap`, and any
other file read tax separately.

## E3. Mem-Size Sweep

Gates: `m80-jp6ik.29`.

Purpose: decide whether the default guest RAM size can move from 1024 MiB to
512 MiB without trading away the workload envelope m80 needs.

Invocation:

```sh
SWEEP=mem_mib SWEEP_VALUES=256,512,1024,2048 KIND=minimal SKIP_LOADED=1 N=30 \
  ./scripts/bench-cold-launch.sh
```

Artifacts:

- `crates/m80-firecracker/benches/snapshots/mem-size-sweep.json`
- `crates/m80-firecracker/benches/sweep-mem_mib.csv`
- `docs/perf/mem-sizing.md`

Required interpretation: phase_12b_ready_accept P50/P95 by memory size,
snapshot file size by memory size, density memory commitment at C=8, and a
plain workload-coverage rationale for the chosen default.

## E4. Loaded-Host Bench

Gates: `m80-jp6ik.25`.

Purpose: quantify the addressable host-scheduler headroom before considering
boot-phase `SCHED_FIFO` elevation.

Invocation:

```sh
STRESS_PROCS="$(nproc)" N=50 KIND=minimal ./scripts/bench-cold-launch.sh
```

If a dedicated background-stress mode is added, record the exact knob here and
in `docs/perf/loaded-host.md`.

Artifacts:

- `crates/m80-firecracker/benches/snapshots/loaded-host-N50.json`
- `docs/perf/loaded-host.md`

Required interpretation: idle vs loaded phase_12b_ready_accept P50/P95/P99,
success rate, failure signatures if any, and the percentage of loaded-host delta
that a scheduler experiment must recover to justify the complexity.

## E5. TLB And Cache-Miss Counters

Gates: `m80-jp6ik.10`, `m80-jp6ik.27`.

Purpose: decide whether hugepages and CPU-template changes have enough
phase_12b headroom to justify implementation.

Invocation:

```sh
PERF_STAT=1 N=20 KIND=minimal SKIP_LOADED=1 ./scripts/bench-cold-launch.sh
```

Host prerequisites: `perf` installed and `sudo perf stat` permitted. On hosts
with `kernel.perf_event_paranoid > 1`, the harness still works when sudo grants
the needed perf capability.

Artifacts:

- `crates/m80-firecracker/benches/perf-counters.csv`
- `docs/perf/tlb-pressure.md`

Required interpretation: per-launch dTLB-load-misses, iTLB-load-misses, and
cache-misses histogram during phase_12b. If dTLB-load-misses are below 100K per
launch, the hugepages payoff is bounded. If they exceed 1M per launch, there is
meaningful headroom.

## E6. Dirty-Page Fraction

Gates: `m80-jp6ik.19`.

Purpose: measure whether diff snapshots are materially smaller than full
snapshots after pristine boot and after a representative exec.

Invocation:

```sh
./scripts/bench-dirty-page-fraction.sh
```

This experiment depends on the `track_dirty_pages` machine-config field
existing. If that field is scaffolded as part of the diff-snapshot bead, keep
the scaffolded implementation open until the real dirty-fraction artifact is
committed.

Artifacts:

- `crates/m80-firecracker/benches/dirty-fraction.csv`
- `docs/perf/dirty-page-baseline.md`

Required interpretation: full snapshot size, diff snapshot size, dirty pages,
total pages, and dirty fraction for pristine post-ready and representative
post-exec states.

## E7. CPU Governor Sweep

Gates: `m80-jp6ik.40`.

Purpose: distinguish hosts where CPU governor tuning matters
(`acpi-cpufreq` with non-performance governor) from hosts where hardware P-state
management makes the advisory irrelevant.

Invocation:

```sh
CPU_GOVERNOR=performance N=50 KIND=minimal SKIP_LOADED=1 \
  ./scripts/bench-cold-launch.sh
CPU_GOVERNOR=ondemand N=50 KIND=minimal SKIP_LOADED=1 \
  ./scripts/bench-cold-launch.sh
```

Artifacts:

- `crates/m80-firecracker/benches/snapshots/cpu-governor-sweep.json`
- `docs/perf/cpu-governor.md`

Required interpretation: scaling driver, governor, phase_12b_ready_accept
P50/P95/P99, and whether the host is an `acpi-cpufreq` host where advisory text
should fire.

## E8. Tokio Runtime Wallclock

Gates: `m80-jp6ik.31`.

Purpose: measure whether per-launch Tokio current-thread runtime construction
is large enough to justify replacing async rtnetlink with synchronous netlink.

Invocation:

```sh
PHASE_JSONL=crates/m80-firecracker/benches/tokio-runtime-cost.jsonl \
  N=20 KIND=minimal SKIP_LOADED=1 ./scripts/bench-cold-launch.sh
```

This requires temporary instrumentation in:

- `crates/m80-guestd/src/pid_one_network.rs`
- `crates/m80-net-outbound/src/link_ops.rs`

Artifacts:

- `crates/m80-firecracker/benches/tokio-runtime-cost.jsonl`
- `docs/perf/tokio-runtime-cost.md`

Required interpretation: P50/P95 for each runtime-construction call site. If
the measured cost is below 3 ms, reconsider the replacement bead. If it exceeds
7 ms, the replacement is justified.

## E9. Kernel Cmdline And Config Delta

Gates: `m80-jp6ik.5`, `m80-jp6ik.6`, `m80-jp6ik.23`.

Purpose: attribute cold-boot wins from stripped-kernel command-line and Kconfig
changes without keeping zero-gain kernel knobs.

Invocation:

```sh
N=50 KIND=minimal SKIP_LOADED=1 KERNEL_KIND=stock ./scripts/bench-cold-launch.sh
N=50 KIND=minimal SKIP_LOADED=1 KERNEL_KIND=stripped ./scripts/bench-cold-launch.sh
```

For staged comparisons, run one cell per variant:

- baseline stripped kernel
- `nokaslr nosmp maxcpus=0`
- `CONFIG_SMP=n` plus stripped debug sections
- the additional HZ, preemption, initrd, scheduler-debug, debug-info, and
  printk-time settings

Artifacts:

- `crates/m80-firecracker/benches/snapshots/kernel-boot-delta.json`
- `docs/perf/kernel-cmdline.md`
- `docs/perf/kernel-config-smp.md`
- `docs/perf/kernel-config-additional.md`

Required interpretation: phase_12b_ready_accept P50/P95/P99 per variant,
kernel size before/after, boot success rate, and per-subphase attribution when
guest boot decomposition is available. Roll back any flag or config setting
that does not measure positive.

## E10. Cargo Release Profile Delta

Gates: `m80-jp6ik.18`.

Purpose: prove that release-profile tuning reduces binary size without hiding a
runtime regression in launch-path code.

Invocation:

```sh
cargo build --release -p m80-cli
N=50 KIND=minimal SKIP_LOADED=1 ./scripts/bench-cold-launch.sh
```

Run the same commands before and after the profile change. Keep test builds on
`panic = "unwind"` so test diagnostics do not degrade.

Artifacts:

- `crates/m80-firecracker/benches/snapshots/release-profile-delta.json`
- `docs/perf/release-profile.md`

Required interpretation: `target/release/m80` size before/after, release build
success, test-profile panic behavior, and phase_1_run_root_prep plus wallclock
P50/P95 before/after. A binary-size win alone is useful, but any runtime
regression must be named.

## E11. KVM Halt-Poll Advisory

Gates: `m80-jp6ik.30`.

Purpose: keep the KVM host-tuning bead scoped as visibility unless a real
measurement later promotes it to a latency claim.

Invocation:

```sh
m80 preflight
```

Optional measurement, only if the bead is promoted from advisory to measured
latency work:

```sh
N=50 KIND=minimal SKIP_LOADED=1 ./scripts/bench-cold-launch.sh
```

Artifacts:

- `docs/ops/host-tuning.md`
- optional: `docs/perf/kvm-halt-poll.md`

Required interpretation: preflight surfaces the current
`/sys/module/kvm/parameters/halt_poll_ns` value and the ops doc explains
latency-priority versus density-priority settings. Do not claim a measured
latency win without a committed before/after bench artifact.

## E12. CPU Template Delta

Gates: `m80-jp6ik.27`.

Purpose: measure the small phase_12a/early-boot effect of removing the default
Intel `T2` CPU template while preserving same-host snapshot behavior.

Invocation:

```sh
N=50 KIND=minimal SKIP_LOADED=1 ./scripts/bench-cold-launch.sh
```

Artifacts:

- `crates/m80-firecracker/benches/snapshots/cpu-template-delta.json`
- `docs/perf/cpu-template.md`

Required interpretation: machine-config PUT omits `cpu_template` by default,
phase_12a_instance_start P50 before/after, same-host snapshot/restore result,
and the documented tradeoff that future cross-host restore needs explicit
CPU-feature parity verification.

## Attribution Rules

Every gated bead that cites this playbook should name the experiment section in
its acceptance criteria, for example:

```text
Verified per docs/perf/measurement-playbook.md#e3-mem-size-sweep:
docs/perf/mem-sizing.md @ <commit>.
```

The section link answers "what protocol produced this number". The artifact
path answers "where is the number". The commit answers "which repo state
produced and interpreted it".
