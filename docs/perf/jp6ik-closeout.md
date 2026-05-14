# m80-jp6ik closeout

Bead: `m80-jp6ik`.

## Intent

`m80-jp6ik` was a measurement-led perf hardening pass after the first
20-agent candidate sweep. Its useful outcome is not "implement every plausible
Firecracker optimization"; it is to separate real launch-path wins from
speculative work, preserve the fixes that reduce concrete overhead or
correctness risk, and leave heavyweight future directions out of the v0.1 ship
bar unless measurements justify pulling them forward.

That framing matches `TARGET.md`: the active v0.1 bar is smoke plus the
privileged real-KVM battery. Perf bench coverage and future direction work are
deferred until the baseline is routinely green. This epic therefore closes on
committed evidence, explicit misses, and named deferrals rather than on broad
new architecture.

## What The Evidence Says

- The no-egress cold path is no longer the first density wall through C=16.
  `docs/perf/density-extended.md` records C=1/C=16 wall P50 at 1516/1517 ms
  with P99 1518/1616 ms.
- Outbound networking is still the product-significant density wall. The same
  artifact records outbound C=1/C=16/C=32 wall P50 at 2316/8120/13925 ms and
  high failure counts at C=16 and C=32.
- The big residual single-VM cost remains boot/readiness, not micro-overhead:
  many candidates landed as tiny correctness or overhead cleanups, but their
  measured launch-path impact was near-zero or below their own gates.
- Snapshot and warm-pool improvements are real future leverage, but the large
  UFFD, diff-snapshot, parent-manifest, and dirty-page-measurement work is a
  future-direction lane, not a v0.1 unblocker.

## Closed Heavy Candidates

`m80-jp6ik.12` implemented iptables-restore batching and kept the iptables
contract intact, but density evidence still points at outbound setup. The
correct interpretation is "batched backend landed; no clean density victory
claimed".

`m80-jp6ik.24` implemented explicit `cpuset.cpus` propagation and warm-pool
slot allocation. Root/KVM verification proved both direct cgroup enforcement
and disjoint warm-pool slot visibility. No density P99 win is claimed because
there is no committed pinned-density artifact.

`m80-jp6ik.7`, `.19`, `.20`, and `.47` stay conceptually valid but are closed
out of this tranche as deferred future-direction work:

- UFFD restore needs a fault-handler lifecycle and failure policy.
- Diff snapshots need `track_dirty_pages`, parent snapshot identity, restore
  validation, and dirty-fraction artifacts.
- nftables needs a policy-backend split and a C=32 outbound density proof after
  the iptables path is no longer enough.

Those are larger architecture tasks. Pull them forward only when the active
target changes from "prove the v0.1 baseline" to "resume perf scale work".

## Review Result

The broader project intent is a generic, hard-cutover Firecracker sandboxing
library with a small honest contract: boot, optionally attach workspace and
egress, exec, report, and tear down without residue. This perf epic served
that intent when it removed measured overhead, fixed real recovery/cgroup bugs,
or rejected low-payoff complexity. It would work against that intent to merge
UFFD, nftables, or diff snapshots as speculative machinery while the ship bar
still depends on basic privileged regression stability.
