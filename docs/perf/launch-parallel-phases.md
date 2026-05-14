# Launch Phase Parallelism Experiment

Bead: `m80-jp6ik.14`

Decision: do not land default launch-phase parallelism.

The experiment temporarily overlapped independent host-side launch work in two
bounded windows:

- `phase_parallel_3_5_6`: storage prep, cgroup probe, and network realize.
- `phase_parallel_5b_10`: cgroup creation and Firecracker UDS open.

The code was removed after measurement because the required NoEgress wallclock
P50 drop did not appear. No outbound run was taken after this result; the bead
requires both NoEgress and outbound wins, so missing the NoEgress bar is enough
to reject the added launch-path complexity.

## Run

Command:

```sh
N=50 KIND=minimal ./scripts/bench-cold-launch.sh
```

Artifact:

- `crates/m80-firecracker/benches/snapshots/launch-parallel-noegress-N50.json`
- source run snapshot: `crates/m80-firecracker/benches/snapshots/2026-05-14T00:00:50+00:00.json`
- artifact sha256: `bae751b778002cca9f1a690672209e8800168e5fc420ac8705e72aed91c32dbf`

Host and substrate match `docs/perf/loaded-host.md`: `vulcan`, Linux
`6.17.0-22-generic`, 48 CPUs, Firecracker/Jailer `v1.15.1`, minimal stock
image, no page-cache dropping beyond the bench harness default.

## Result

Baseline is `docs/perf/loaded-host.md` / `loaded-host-N50.json`.

| cell | baseline successes | experiment successes | baseline failures | experiment failures | baseline wallclock P50 | experiment wallclock P50 | baseline `phase_12b_ready_accept` P50 | experiment P50 |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| minimal idle | 50 | 49 | 0 | 1 | 1338 ms | 1339 ms | 1010.858 ms | 1012.718 ms |
| minimal loaded | 50 | 50 | 0 | 0 | 1488 ms | 1492 ms | 1052.150 ms | 1049.170 ms |

The bead required cold-launch P50 to drop by at least 10 ms in NoEgress. The
idle cell regressed by 1 ms and had one failed launch. The loaded cell
regressed by 4 ms wallclock P50, even though `phase_12b_ready_accept` improved
by 2.980 ms.

The aggregate phase markers prove the experiment path ran:

| phase | idle P50 | loaded P50 |
|---|---:|---:|
| `phase_parallel_3_5_6` | 12.874 ms | 17.073 ms |
| `phase_parallel_5b_10` | 19.887 ms | 18.125 ms |

## Follow-Up

The rejected path shows that this overlap does not buy measurable NoEgress
wallclock latency on the current launch shape. Revisit only if a later change
makes one of these host-side phases both large and independent enough to clear
the P50 bar with failure-free runs.
