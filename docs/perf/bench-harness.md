# Bench harness reference

`scripts/bench-cold-launch.sh` is the front door to the m80 perf-bench
push. It runs N cold launches per cell, captures end-to-end wallclock
and per-phase timings, emits a JSON snapshot, and feeds
`scripts/bench-summary.py` for percentiles, histograms, confidence
intervals, sweeps, concurrent aggregation, and regression-gating diffs.

Experiment-specific protocols live in
[`docs/perf/measurement-playbook.md`](measurement-playbook.md). Use that file
when a bead needs a real-substrate close gate rather than just harness plumbing
coverage.

## Quick start

```sh
# Default: 30 launches per (ubuntu|minimal) × (idle|loaded) cell.
./scripts/bench-cold-launch.sh

# Tail latency: N=1000 with cache drops + idle-only.
N=1000 SKIP_LOADED=1 ./scripts/bench-cold-launch.sh --cold-isolation

# Sweep a single variable.
SWEEP=vcpu SWEEP_VALUES=1,2,4 ./scripts/bench-cold-launch.sh

# Concurrent admission probe.
CONCURRENT=4 ./scripts/bench-cold-launch.sh

# Concurrent outbound admission probe.
EGRESS=outbound CONCURRENT=4 ./scripts/bench-cold-launch.sh

# Best-effort host isolation.
TASKSET=0-3 CPU_GOVERNOR=performance ./scripts/bench-cold-launch.sh

# Dry-run prints the plan without spawning anything.
./scripts/bench-cold-launch.sh --dry-run
```

## Knob reference

| Env var | Default | What |
|---|---|---|
| `N` | 30 | samples per cell (post-warmup). `N=1000` for P99.9. |
| `WARMUP` | 2 | warmup attempts per cell that are discarded from stats. |
| `KIND` | both | `ubuntu`, `minimal`, or `both`. |
| `KERNEL_KIND` | stock | `stock` or `stripped`. |
| `EGRESS` | none | `none` or `outbound`. Falls back to `M80_NETWORK_POLICY` when set. |
| `SKIP_LOADED` | 0 | skip the stress-ng cell. |
| `STRESS_PROCS` | `$(nproc)` | `--cpu N` passed to stress-ng for the loaded cell. |
| `SWEEP` | — | sweep one of `vcpu`, `mem_mib`, `kernel_kind`, `image_kind`. |
| `SWEEP_VALUES` | per-var defaults | comma-separated values to iterate. |
| `CONCURRENT` | 0 | N parallel admissions per attempt; emits `concurrent.csv`. |
| `TASKSET` | — | passed to `taskset -c` on every m80 invocation. |
| `CPU_GOVERNOR` | — | passed to `cpupower frequency-set -g` once at start. |
| `PHASE_JSONL` | — | when set, appends one JSON line per phase event for flamegraphs. |
| `BENCH_ARTIFACT_DIR` | `crates/m80-firecracker/benches` | directory for CSVs and snapshots; used by harness tests to isolate artifacts. |
| `M80_BIN` | `./target/release/m80` | binary path. Override for testing. |

## Flags

| Flag | What |
|---|---|
| `--cold-isolation` | `sync && echo 3 > /proc/sys/vm/drop_caches` between every run (true cold-cold). |
| `--dry-run` | Print the plan and exit; no I/O, no sudo, no cargo. |
| `--help`, `-h` | Show usage. |

## Why warmup matters

The first 2–3 launches per cell are colder than the steady state because:

- The kernel's page cache hasn't yet faulted in firecracker, the jailer,
  the kernel image, and the rootfs.
- TLB entries are cold; on Intel this costs ~50–200 µs per cache miss in
  the boot path.
- The kernel scheduler hasn't yet sized its run-queue for our workload.

Warmup attempts are discarded from wallclock rows, per-phase rows, JSONL
phase events, and computed snapshots. If you want to *measure the
cold-cold case explicitly* (e.g. you care about the very first launch
latency after a fresh host boot), use `--cold-isolation` and set
`WARMUP=0`.

## Per-phase JSON event stream

`PHASE_JSONL=<path>` appends one JSON line per phase event:

```jsonl
{"timestamp_ns":1715534441536740708,"kind":"minimal","kernel_kind":"stock","load":"idle","attempt":1,"phase":"phase_1_run_root_prep","elapsed_us":13173}
```

The timeline can be loaded into flamegraph/perfetto tooling for per-phase
visualization, or processed line-by-line by `jq` for ad-hoc analysis:

```sh
jq -s 'group_by(.phase) | map({phase: .[0].phase, p99_us: (sort_by(.elapsed_us) | .[(length*99/100|floor)].elapsed_us)})' < events.jsonl
```

## Sweep mode

`SWEEP=<var>` produces `crates/m80-firecracker/benches/sweep-<var>.csv`
with one wallclock row per attempt × sweep value, and
`crates/m80-firecracker/benches/sweep-<var>-phases.csv` with the same
`sweep_var` / `sweep_value` attribution for per-phase rows. Currently
supported variables:

- `vcpu` — passes `--vcpu-count <value>` to `m80 run`.
- `mem_mib` — passes `--mem-size-mib <value>` to `m80 run`.
- `kernel_kind` — switches the boot kernel between `stock` and `stripped`.
- `image_kind` — switches between `ubuntu` and `minimal`.

`scripts/bench-summary.py sweep` aggregates those CSVs into per-value
wallclock and phase percentiles.

## Concurrent mode

`CONCURRENT=N` spawns N parallel m80 invocations per attempt and measures
wall-time-to-all-ready (the per-attempt max launch time across the N
parallel VMs). Output goes to `concurrent.csv`. The
`compute_concurrent` helper exposes:

- `wall_time_to_all_ready_p50_ms` (and p95): the headline scaling number
- `per_vm_p50_ms` / `per_vm_p95_ms` / `per_vm_p99_ms`: tail of individual
  VM launches under concurrent load

For the extended density experiment, use
`scripts/bench-density-extended.sh`. It runs the configured no-egress and
outbound ladders through `bench-cold-launch.sh`, then writes a dedicated
`crates/m80-firecracker/benches/density-extended.csv` and
`crates/m80-firecracker/benches/snapshots/density-extended.json` so the
capacity artifact is not hidden inside the append-only `concurrent.csv`.

For the cold-cold snapshot restore experiment, use
`scripts/bench-restore-cold.sh`. It builds the real-KVM restore bench, runs
N warm-cache restores and N dropped-cache restores, then writes
`crates/m80-firecracker/benches/snapshots/cold-restore-N${N}.json` with
restore phase percentiles and direct `vm.snap` / `mem.snap` file-read timing.

## Confidence intervals + outlier flagging

Every wallclock cell snapshot carries:

- `p50_ci95`: bootstrap 95% confidence interval for the median, drawn
  from 200 resamples with a fixed RNG seed (deterministic per
  invocation).
- `outliers`: count of samples more than 2σ from the mean.

The CI widens with sample variance; a tight CI (e.g. `[12, 13]` ms) means
the next run is very likely to produce the same P50 within ±1 ms. An
outlier count > ~3% of N is a signal that the run was disturbed (host
contention, transient pages-out, etc.) and should be re-run.

## Regression gate

`scripts/bench-summary.py diff baseline.json new.json --fail-on-regress 10`
exits non-zero if any phase's `delta_pct > 10%`. CI integration:

```yaml
- name: bench
  run: N=200 SKIP_LOADED=1 ./scripts/bench-cold-launch.sh
- name: regression-gate
  run: |
    python3 scripts/bench-summary.py diff \
        crates/m80-firecracker/benches/baseline.json \
        crates/m80-firecracker/benches/snapshots/latest.json \
        --fail-on-regress 10
```

Re-baseline manually after a deliberate perf change:

```sh
cp crates/m80-firecracker/benches/snapshots/latest.json \
   crates/m80-firecracker/benches/baseline.json
git add ... && git commit -m "perf: snapshot new baseline (<reason>)"
```

## Inline reference

| Run mode | Knob | Exercises |
|---|---|---|
| sequential | (default) | `cold-launch.csv`, `cold-launch-phases.csv` |
| tail-latency | `N=1000` | extended percentiles, histograms |
| sweep | `SWEEP=vcpu` | `sweep-<var>.csv`, `compute_sweep` |
| concurrent | `CONCURRENT=N` | `concurrent.csv`, `compute_concurrent` |
| restore cold-cold | `bench-restore-cold.sh` | restore warm/cold JSON artifact |
| cold-cold | `--cold-isolation` | drop_caches per run |
| isolated | `TASKSET=` + `CPU_GOVERNOR=` | best-effort host pinning |
| trace | `PHASE_JSONL=` | flamegraph-ready event stream |
| dry-run | `--dry-run` | plan-only |

## bench-extras.sh sub-modes (B1-B10)

`scripts/bench-extras.sh --MODE` orchestrates the later perf sub-epics
against the same harness. Each mode emits a CSV + prints a one-line
summary. Each mode requires sudo and the same `/opt/firecracker/bin/`
binaries the smoke test uses.

| Mode | Sub-epic | What it produces |
|---|---|---|
| `--throughput` | B2 | `throughput.csv` (ops/sec sustained over `WINDOW_SEC`) |
| `--memory` | B4 | `memory.csv` (RSS samples of live firecracker processes) |
| `--teardown` | B8 | `teardown.csv` (stop_bounded / residue_cleanup / release breakdown) |
| `--boot-decomp` | B1 | `boot-decomp.txt` (guest `dmesg`; grep `initcall` for per-call us) |
| `--long-tail` | B10 | console report of P99/P99.9 from `snapshots/latest.json` |
| `--density` | B3 | iterates `bench-cold-launch.sh CONCURRENT=$c` over `$LADDER` |
| `bench-density-extended.sh` | m80-jp6ik.42 | no-egress/outbound C=1..64 density artifact |
| `bench-restore-cold.sh` | m80-jp6ik.43 | warm-cache and cold-cache snapshot restore artifact |

Compute helpers in `scripts/bench-summary.py`:

- `compute_throughput(csv)` — `{kind: {op: {ops_per_sec, p50_ms, ...}}}`
- `compute_memory(csv)` — `{rss_kb_p50, rss_kb_p95, sample_count, vm_count_p50}`
- `compute_teardown(csv)` — `{kind: {phase: {p50_us, p95_us, ...}}}`

All three are unit-tested via `python3 scripts/bench-summary.py --test`.

## Test coverage matrix

| Layer | Test | Verifies |
|---|---|---|
| compute logic | `python3 scripts/bench-summary.py --test` | 46 unit tests on percentiles, histograms, CIs, outliers, sweep, concurrent, throughput, memory, teardown, diff |
| shell orchestration | `bash scripts/test-bench-harness.sh` | 30 e2e: help flags, --dry-run, all env vars, --cold-isolation, bench-extras modes |
| harness real-KVM | `N=200 SKIP_LOADED=1 ./scripts/bench-cold-launch.sh` on a privileged host | full e2e: launches, CSVs, snapshot compute, diff with --fail-on-regress |
| bench-extras real-KVM | `./scripts/bench-extras.sh --MODE` on a privileged host | each B1-B10 sub-epic mode end-to-end |

## What remains for the privileged runner (m80-16hx7)

Code in this directory is exercised against real KVM via
`./scripts/bench-cold-launch.sh` on a privileged host. To produce
data for capacity claims and regression gates:

1. Provision the privileged runner per `m80-16hx7` (artifact cache + KVM).
2. Run `N=1000 SKIP_LOADED=1 ./scripts/bench-cold-launch.sh --cold-isolation`
   to seed `baseline.json` with real measurements.
3. Cycle through `scripts/bench-extras.sh --MODE` for each sub-epic and
   commit the resulting snapshots to `docs/perf/<area>.md` as the
   reference numbers backing the capacity claims.
