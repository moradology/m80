# CPU Governor Sweep

Date: 2026-05-14

Bead: `m80-jp6ik.40`

Artifact: `crates/m80-firecracker/benches/snapshots/cpu-governor-sweep.json`

## Setup

Host:

- Linux 6.17.0-22-generic
- scaling driver: `acpi-cpufreq`
- original governor before the sweep: `schedutil`
- available governors: `conservative`, `ondemand`, `userspace`, `powersave`,
  `performance`, `schedutil`

Benchmark command:

```sh
CPU_GOVERNOR=<governor> N=50 KIND=minimal SKIP_LOADED=1 \
  ./scripts/bench-cold-launch.sh
```

The sweep ran `performance` and `ondemand`, then restored the original
`schedutil` governor.

## Results

| Governor | Wall P50 | Wall P95 | Wall P99 | Failures |
| --- | ---: | ---: | ---: | ---: |
| `performance` | 1236 ms | 1237 ms | 1238 ms | 0/50 |
| `ondemand` | 1351 ms | 1353 ms | 1354 ms | 0/50 |

Selected phase P50s:

| Governor | `phase_12b_ready_accept` | `phase_9_jailer_launch` | `phase_11_rest_puts` | `phase_12a_instance_start` |
| --- | ---: | ---: | ---: | ---: |
| `performance` | 989.575 ms | 15.497 ms | 1.075 ms | 13.936 ms |
| `ondemand` | 993.573 ms | 31.685 ms | 1.762 ms | 15.345 ms |

## Interpretation

On this `acpi-cpufreq` host, `performance` improved minimal/idle cold-launch
wallclock P50 by `115 ms` versus `ondemand` (`1236 ms` vs `1351 ms`). The
`phase_12b_ready_accept` P50 moved only `3.998 ms`; the larger wallclock win is
spread across host-side launch work and scheduler/frequency behavior outside
the kernel-ready bucket.

This supports the advisory shape implemented for `m80-preflight`: warn on
`acpi-cpufreq` hosts that are not already using `performance`, while keeping
the row non-blocking and not warning for P-state drivers where the governor
name alone is less meaningful.
