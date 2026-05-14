# BlankPool Decision

Bead: `m80-jp6ik.26`

Decision: do not implement a pre-forked Firecracker `BlankPool` in this
perf tranche.

## Context

`BlankPool` would hold Firecracker processes after jailer launch but before
machine configuration, device PUTs, `InstanceStart`, or guest readiness. That
keeps per-launch parameter freedom and avoids WarmPool's snapshot memory cost,
but the pool can remove only the host process-launch slice. It still pays
storage prep, cgroup attach, REST PUTs, `InstanceStart`, and the full guest
kernel/PID-1 ready path on every request.

The implementation is not a small WarmPool variant. `WarmPool` owns
pre-restored `RunningSandbox` values. A blank slot would require a new
intermediate lifecycle state: run-root, ownership lease, storage artifacts,
jailer materialization, live Firecracker process, API socket client, cleanup
guards, and possibly cgroup state, but no configured VM. That would split the
current `Sandbox::launch` pipeline across a public `configure_and_start` seam.

The existing launch path also binds scratch drives during
`phase_4_jailer_materialize`. The simpler blank-slot shape would preallocate
scratch at fill time even for no-workspace requests. The cleaner shape would
extend the jailer bind-mount API to support post-fork additions, which is a
larger jailer/lifecycle refactor than this secondary perf epic should carry.

## Evidence

Current committed measurements put the removable phase below the size needed
to justify that lifecycle split:

| Evidence | Relevant result |
|---|---:|
| `docs/behaviors/jailer/pid-file-backoff.md` | `phase_9_jailer_launch` P50 moved from 25,437 us to 15,588 us after the pid-file backoff fix. |
| `docs/perf/launch-parallel-phases.md` | Overlapping nearby host-side phases missed the NoEgress bar and was rejected for added launch-path complexity. |
| `docs/perf/minimal-erofs.md` | Current stripped-kernel minimal ext4 and erofs wallclock P50 is about 1340-1355 ms. |

The original BlankPool acceptance expected `try_lease -> configure_and_start
-> exec` within 1.5-1.6 s P50. The current no-pool cold path is already below
that band on the same perf tranche, while BlankPool would add a resident owner,
slot lifecycle, health replacement, memory accounting, CLI surface, and a new
half-launched cleanup state to save at most the remaining roughly 15.6 ms
`phase_9_jailer_launch` P50.

## Follow-Up Trigger

Reopen this idea only if one of these changes:

- `phase_9_jailer_launch` regresses above 50 ms P50 in a supported host mode.
- A caller needs many cold, per-launch-configurable VMs but cannot use
  snapshot/WarmPool for semantic reasons, and accepts resident blank-process
  memory as an operational feature.
- The launch lifecycle is already being split for a non-perf reason, making
  a blank configured-start seam effectively free.

Until then, the broader intent of `m80-jp6ik` is better served by measured
kernel/ready-path work, storage-prep work, or snapshot/WarmPool improvements
that attack hundreds of milliseconds rather than a low-double-digit host slice.
