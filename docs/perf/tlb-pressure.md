# TLB and cache-miss baseline

Bead: `m80-jp6ik.46`

This is the E5 measurement from `docs/perf/measurement-playbook.md`. It decides
whether the hugepages candidate (`m80-jp6ik.10`) has enough measured TLB
pressure to justify adding a `huge_pages` configuration path.

## Run

Command:

```sh
PERF_STAT=1 PERF_STAT_SECONDS=2 N=20 KIND=minimal SKIP_LOADED=1 \
  ./scripts/bench-cold-launch.sh
```

`WARMUP=2` was left at the default. Attempts 1 and 2 warmed the host and were
discarded from wallclock, phase, and perf-counter artifacts. The committed
`perf-counters.csv` contains attempts 3 through 22.

Artifacts:

- `crates/m80-firecracker/benches/perf-counters.csv`
- `crates/m80-firecracker/benches/snapshots/tlb-pressure-N20.json`
- perf CSV sha256: `dfb2bb172749a73a2aad77156289e27f1a33e1ec9a2b3eeef90e52c9a620e2a7`
- snapshot sha256: `828e21e73c2c5ecd0ed90a0b0159d1deff6f40f4a1f8b112df972148fdd24429`

Host and substrate:

- host: `vulcan`
- kernel: `Linux 6.17.0-22-generic #22-Ubuntu SMP PREEMPT_DYNAMIC Fri Mar 13 12:04:44 UTC 2026 x86_64`
- CPUs: 48
- perf: `perf version 6.17.13`
- `kernel.perf_event_paranoid`: `4`; unprivileged perf was denied, but `sudo perf` succeeded and is what the harness uses
- Firecracker/Jailer: `v1.15.1`
- image kind: minimal stock kernel
- image path: `/tmp/m80-build/minimal`
- image hashes:
  - `vmlinux`: `c453f36520d2f2792ab8e4532a814e4a647a4a41a4c94d4e9083a502800159b1`
  - `output.ext4`: `420e8f0797b8c2a8ae1e8e5c89fb5ce1d515cd5148929bbb693d6f7346c6710b`
  - `output.ext4.manifest.json`: `4b35a83d286a80cc7e1b8d4133b56b17b57f30b565aa235e91f2c50e7815c4f9`

The perf window starts as soon as the bench harness observes
`jailer-state.json` and has the Firecracker PID, before the host has completed
the remaining REST setup and `InstanceStart`. It runs for two seconds with
100 ms interval rows. The per-launch totals below sum non-empty interval rows;
intervals after the Firecracker process has exited are preserved in the CSV with
empty counts.

## Launch Shape

| cell | successes | failures | wallclock P50 | wallclock P95 | `phase_12b_ready_accept` P50 | P95 |
|---|---:|---:|---:|---:|---:|---:|
| minimal idle | 20 | 0 | 1345 ms | 1348 ms | 1016.789 ms | 1024.574 ms |

## Counter Histogram

Per-launch totals from `perf-counters.csv`:

| event | min | P50 | P75 | P90 | P95 | max |
|---|---:|---:|---:|---:|---:|---:|
| `dTLB-load-misses` | 35,780 | 40,508 | 42,325 | 43,943 | 44,461 | 48,290 |
| `iTLB-load-misses` | 2,360 | 3,549 | 3,864 | 3,994 | 4,238 | 4,406 |
| `cache-misses` | 318,987 | 559,752 | 759,555 | 830,174 | 833,313 | 1,721,390 |

The dTLB distribution is tightly below the playbook's 100K-per-launch lower
bound. iTLB pressure is much smaller. Cache misses are present, but hugepages
primarily target TLB pressure, not a general cache-miss budget.

## Interpretation

`dTLB-load-misses` P50 is 40.5K and P95 is 44.5K per launch, below the
measurement gate's 100K cutoff. That bounds the expected hugepages win: there
is not enough measured TLB pressure in this boot path to justify adding
`MachineConfig.huge_pages`, preflight reservation checks, host setup docs, and
the associated failure surface as a perf default candidate.

Close `m80-jp6ik.10` as not justified by this host measurement. Reopen or file a
new bead only if a different workload/host records dTLB misses above the
playbook's 1M-per-launch threshold or shows a direct hugepages before/after
win that contradicts this baseline.
