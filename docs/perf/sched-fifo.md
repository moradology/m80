# SCHED_FIFO Boot-Window Experiment

Bead: `m80-jp6ik.25`

Decision: do not land default boot-window `SCHED_FIFO`.

The experiment temporarily elevated the jailed Firecracker process to
`SCHED_FIFO` priority 50 after cgroup enrollment and restored `SCHED_OTHER`
after the inverted guestd ready signal. The guard and syscall wrapper were
removed after measurement because the loaded-host result missed the bead's
justification bar and introduced a failure.

## Run

Command:

```sh
STRESS_PROCS="$(nproc)" N=50 KIND=minimal ./scripts/bench-cold-launch.sh
```

Artifact:

- `crates/m80-firecracker/benches/snapshots/sched-fifo-rejected-N50.json`
- source run snapshot: `crates/m80-firecracker/benches/snapshots/2026-05-13T23:48:02+00:00.json`
- artifact sha256: `f914f6585dfccf7e4fd9ea1193009699311e8859b6f81487043f91ac805cc07a`

Host and substrate match `docs/perf/loaded-host.md`: `vulcan`, Linux
`6.17.0-22-generic`, 48 CPUs, Firecracker/Jailer `v1.15.1`, minimal stock
image, no page-cache dropping beyond the bench harness default.

## Result

Baseline is `docs/perf/loaded-host.md` / `loaded-host-N50.json`.

| cell | baseline successes | experiment successes | baseline failures | experiment failures | baseline wallclock P50 | experiment wallclock P50 | baseline `phase_12b_ready_accept` P50 | experiment P50 |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| minimal idle | 50 | 50 | 0 | 0 | 1338 ms | 1239 ms | 1010.858 ms | 981.328 ms |
| minimal loaded | 50 | 49 | 0 | 1 | 1488 ms | 1494 ms | 1052.150 ms | 1067.824 ms |

The bead required loaded-host cold-launch P50 to drop by at least 40 ms while
idle launch stayed unchanged. The idle cell did not regress, but the loaded
cell worsened by 6 ms wallclock P50, worsened by 15.674 ms in
`phase_12b_ready_accept`, and had one failed launch. The phase markers prove
the experiment path ran (`phase_9b_rt_elevate`, `phase_12c_rt_demote`), but it
did not recover the loaded-host scheduling delta.

## Follow-Up

If this area is revisited, treat real-time scheduling as a new design problem,
not as a ready default. The next attempt should explain what changed in the
safety/performance case before reintroducing scheduler mutation.
