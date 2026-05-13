# 0002 — Bead closure: scaffolded vs verified

## Context

The `m80-ekbk` perf-bench epic false-closed on 2026-05-12: all children were
marked closed even though the then-current bench artifacts were backed by mock
data rather than real measurements from KVM, `sudo`, and the real image
filesystem. The follow-up postmortem is
[`docs/postmortems/2026-05-12-ms-bind-and-sudo-escape.md`](../postmortems/2026-05-12-ms-bind-and-sudo-escape.md).

This is a vocabulary and discipline fix, not yet a tooling fix. The verb
`close` had collapsed two states that matter for measurement-shaped work:

- **scaffolded:** code exists, compiles, has unit coverage, and the
  orchestration can run end-to-end against substitutes.
- **verified:** the code has been exercised against the real production
  substrate and the named observable has been measured and recorded.

For benches, perf snapshots, capacity claims, and smoke gates, scaffolded is
useful but not sufficient.

## Decision

Measurement-shaped beads carry the `requires-verified-close` label. A bead is
measurement-shaped when its acceptance criteria name a numeric observable such
as latency, throughput, RSS, success rate, error rate, density, or capacity.

A `requires-verified-close` bead closes in two steps:

1. `br update <id> --notes "scaffolded: <commit-sha>"` when the code lands and
   the orchestration runs end-to-end against substitutes. The bead remains
   `open` or `in_progress`.
2. `br close <id> --reason "verified: <artifact-path> @ <commit-sha>"` when
   the numeric observable has been measured against the production substrate
   and the result is committed. The reason must cite the artifact path, such
   as `crates/m80-firecracker/benches/baseline.json`, and the commit that
   introduced the real data.

Agents may set scaffolded notes. Only a human, or an agent with explicit
operator confirmation of a real-substrate run, may set verified closure.

Parent epics with at least one `requires-verified-close` child inherit the
label. Verified close of the parent requires verified close of every such
child.

## Consequences

Some agent workflows slow down. Some beads will sit open after code lands
because real KVM, real `sudo`, real network, real filesystem, or a long enough
sample run is not yet available. That is correct. The cost is paid in waiting
time now instead of false production confidence later.

The label convention is review-enforced for now. If it rots, promote the
distinction into `br` as a first-class status or close-eligibility rule.

## Alternatives Considered

**Just be more careful:** rejected. That was the effective policy during the
`m80-ekbk` push, and it failed under pressure from a large scaffolded epic.

**New bead statuses (`scaffolded`, `verified`):** rejected for now because it
requires tracker tooling changes. A label convention is cheap enough to apply
immediately and gives us evidence before tool work.

**Per-epic close-eligibility predicate in `br`:** deferred. It may be the right
tooling shape later, but the immediate failure was vocabulary and review
discipline.
