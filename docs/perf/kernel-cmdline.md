# Kernel cmdline experiment

Date: 2026-05-13

Bead: `m80-jp6ik.5`

## Candidate

The candidate appended these stripped-kernel command-line flags:

```text
nokaslr nosmp maxcpus=0
```

The intent was to remove KASLR relocation and 1-vCPU SMP setup work from the
kernel side of `phase_12b`.

## Baseline

Baseline snapshot:

- `crates/m80-firecracker/benches/snapshots/2026-05-13T15:43:07+00:00.json`

Command:

```sh
N=50 WARMUP=2 KIND=minimal SKIP_LOADED=1 KERNEL_KIND=stripped \
  M80_BIN=./target/release/m80 ./scripts/bench-cold-launch.sh
```

Key baseline values:

| metric | P50 us | P95 us |
|---|---:|---:|
| `phase_12b_ready_accept` | 1,028,202 | 1,039,336 |
| `phase_12b_host_waiting_accept` | 1,026,647 | 1,037,141 |
| `phase_12b_kernel_console_range` | 952,008 | 960,008 |

## Attempt Result

The after-change run used the same command after adding
`nokaslr nosmp maxcpus=0`. It did not reach the acceptance bar. The harness was
still only on attempt 8 after more than seven minutes, where the baseline
completed the full N=50 cell in roughly 75 seconds. The run was terminated and
the flags were reverted.

## Decision

Do not keep these flags in m80's stripped cmdline. On the current kernel/image
path they are not a measured win and are plausibly boot-hostile in combination.
Future kernel-cmdline work should test one flag at a time against the
`phase_12b_kernel_console_range` marker rather than bundling this set.
