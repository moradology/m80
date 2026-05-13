# Loaded-host cold launch baseline

Bead: `m80-jp6ik.45`

This is the E4 loaded-host measurement from
`docs/perf/measurement-playbook.md`. It quantifies how much cold-launch latency
appears when the host is CPU saturated with `stress-ng`, before deciding whether
boot-phase `SCHED_FIFO` is worth its watchdog and demotion complexity.

## Run

Command:

```sh
STRESS_PROCS="$(nproc)" N=50 KIND=minimal ./scripts/bench-cold-launch.sh
```

Artifact:

- `crates/m80-firecracker/benches/snapshots/loaded-host-N50.json`
- source run snapshot: `crates/m80-firecracker/benches/snapshots/2026-05-13T23:05:53+00:00.json`
- artifact sha256: `299c9b12d26e5873f5658fc495cf5b2e88b617d6aebbcf1e24905bdc98d0e1c2`

Host and substrate:

- host: `vulcan`
- kernel: `Linux 6.17.0-22-generic #22-Ubuntu SMP PREEMPT_DYNAMIC Fri Mar 13 12:04:44 UTC 2026 x86_64`
- CPUs: 48
- CPU scaling driver/governor: `acpi-cpufreq` / `schedutil`
- Firecracker/Jailer: `v1.15.1`
- stress-ng: `0.19.03`
- image kind: minimal stock kernel
- image path: `/tmp/m80-build/minimal`
- image hashes:
  - `vmlinux`: `c453f36520d2f2792ab8e4532a814e4a647a4a41a4c94d4e9083a502800159b1`
  - `output.ext4`: `420e8f0797b8c2a8ae1e8e5c89fb5ce1d515cd5148929bbb693d6f7346c6710b`
  - `output.ext4.manifest.json`: `4b35a83d286a80cc7e1b8d4133b56b17b57f30b565aa235e91f2c50e7815c4f9`

No page-cache dropping was used. `WARMUP=2` was the default; wallclock and
per-phase snapshot rows both contain the 50 post-warmup samples only.

## Results

| cell | successes | failures | wallclock P50 | wallclock P95 | wallclock P99 | `phase_12b_ready_accept` P50 | P95 | P99 |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| minimal idle | 50 | 0 | 1338 ms | 1341 ms | 1341 ms | 1010.858 ms | 1016.970 ms | 1018.440 ms |
| minimal loaded | 50 | 0 | 1488 ms | 1499 ms | 1502 ms | 1052.150 ms | 1084.965 ms | 1085.811 ms |

Loaded-host deltas:

| metric | loaded - idle | 60% recovery threshold |
|---|---:|---:|
| wallclock P50 | +150 ms | 90.0 ms |
| wallclock P95 | +158 ms | 94.8 ms |
| wallclock P99 | +161 ms | 96.6 ms |
| `phase_12b_ready_accept` P50 | +41.292 ms | 24.775 ms |
| `phase_12b_ready_accept` P95 | +67.995 ms | 40.797 ms |
| `phase_12b_ready_accept` P99 | +67.371 ms | 40.423 ms |

Largest P50 phase shifts:

| phase | idle P50 | loaded P50 | delta |
|---|---:|---:|---:|
| `phase_12b_host_waiting_accept` | 1008.788 ms | 1048.587 ms | +39.799 ms |
| `phase_9_jailer_launch` | 15.534 ms | 34.042 ms | +18.508 ms |
| `phase_12b_kernel_console_range` | 945.088 ms | 962.585 ms | +17.497 ms |
| `phase_4_jailer_materialize` | 0.802 ms | 11.553 ms | +10.751 ms |

## Interpretation

The loaded cell succeeded 50/50, so this run does not reproduce the older
stress-ng saturation failure mode. Under full CPU stress, end-to-end wallclock
P50 grows by 150 ms, but the specific boot-readiness phase grows by 41.292 ms
at P50 and about 68 ms at P95/P99.

For `m80-jp6ik.25`, boot-phase `SCHED_FIFO` should recover at least 60% of the
`phase_12b_ready_accept` loaded-host delta to justify the watchdog complexity:
about 24.8 ms at P50 or 40.8 ms at P95. If the experiment cannot recover that
much on this host, the scheduler elevation should be rejected or narrowed to a
lower-risk advisory rather than landed as a default boot path.
