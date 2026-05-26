# m80 — Current Target

This file is the single source of truth for **what m80 is working toward right
now**. It complements (does not replace) the scope boundary in `AGENTS.md`,
the per-crate contracts in `crates/<name>/README.md`, and the granular work
in `.beads/`. It is meant to be edited freely as priorities shift; treat
drift here as a bug.

## Driving consumer

m80 has a specific external adapter consumer driving v0.1 priority. The
consumer maps **LLM tool calls** onto m80's exec / file-op / lifecycle
primitives — m80 is the "hands" that execute tool calls inside isolated
microVMs; the consumer (and the LLM behind it) chooses *which* tool calls.

The consumer lives outside this repo and is not named here. m80 stays
generic per `docs/adapter-boundary.md`. What matters for m80's priority is
the consumer's *profile*:

- Many short-lived sandboxes per session → fast launch latency matters
  (warm pool, snapshot restore)
- Multi-tenant load → strong isolation + concurrent-VM reliability
- Untrusted LLM-generated input running inside the sandbox → guest-escape
  resistance is the security bar
- Programmatic `read_file` / `write_file` / `list_dir` / `stat` for moving
  workload artifacts in and out → file-op primitives must be typed and
  reliable, not shell-quoted exec
- Transparent stdout / stderr / exit code → m80's "thin host-process
  wrapper" promise from the README is load-bearing

The consumer's specific tool catalog, idempotency contract, semantic IDs,
and event vocabulary stay above m80, in the consumer's own repo, per the
adapter boundary.

## Ship bar (v0.1)

Two gates, both must be green at the same time:

1. **`scripts/smoke.sh` green on `minimal` and `ubuntu` image kinds, with
   multi-VM concurrent runs.** This is the cold-launch + exec + stop +
   writeback + delete loop — the m80 baseline. If smoke is flaky, nothing
   downstream of it is reliable.
2. **All real-KVM `#[ignore]` tests pass on a privileged runner.** This
   includes the full cluster: `end_to_end_real_kvm`, `egress_outbound_real_kvm`,
   `fileop_errors_real_kvm`, `pty_interaction_real_kvm`, `stop_disposition`,
   `lifecycle_failure`, `cgroup_memory_oom`, `concurrency`,
   `run_dir_invariants`, `wire_frame_boundaries`, `warm_pool`,
   `persistent_state`, `idle_timeout`, `cancellation`, `streaming_exec`,
   `overlay_lifecycle`, `diagnostics_phase_markers`, `join_netns`,
   `layer1_guest_smoke`, `egress_none`.

No calendar deadline. **ASAP** = land what's required for the bar; defer
what isn't. Don't add nice-to-haves before the bar is met.

## Top of stack (active)

In priority order. Update freely as work lands.

1. **`m80-16hx7.7` + `m80-s3r28.2` + `m80-16hx7.12` — self-hosted CI
   workflow and battery convergence.** Wire the
   now-documented runner, cleanup, and JSON report into an operator-triggered
   privileged CI lane; close only after a verified self-hosted runner run.
   Current state: the L1 self-hosted runner, substrate check, and real-KVM
   `scripts/smoke.sh` path are proven green. The full ignored-test battery is
   now the blocker; treat its JSON report as the source of truth for what to
   fix next.
2. **`m80-16hx7.2` — artifact build/cache, conditional.** Keep this as a
   runtime improvement unless repeated real-KVM battery runs are too slow or
   too hand-wired to use without a reproducible cache.

Closed proof chain: `m80-16hx7.9`, `m80-16hx7.10`, and `m80-16hx7.11`
produced the nested-KVM public-install smoke artifact for `v0.2.20`.
`m80-16hx7.1`, `.3`, and `.4` are now closed as of 2026-05-25: runner setup is
documented/idempotent, ignored tests have enforced structured reasons, no-KVM
and privileged selector proofs are green, and the pre-test reaper has seeded
real-host residue coverage. `m80-16hx7.6` now has a documented/validated JSON
report schema for the wrapper output. `m80-16hx7.5` now adds per-test
before/after leak diffs, reports newly leaked host state as `leak-check`, and
has a live wrapper proof against the helper package's ignored reaper test.

## Long-run order

This is the working order when someone asks to keep pushing for a long stretch.
Do not fan this into dozens of new leaves unless the report proves independent
root causes.

1. **Make the privileged battery converge.** Finish `m80-16hx7.7` /
   `m80-s3r28.2` by driving the self-hosted workflow from "smoke green" to
   "full report green or each remaining failure has a specific product/test
   bead." Prefer fixing stale tests that contradict the current crate contract
   over weakening the product. In particular, tests that assume root-capable
   guest exec must either be rewritten around the documented non-root guestd
   profile or moved into a dedicated root/probe harness with an explicit ignore
   token.
2. **Only then decide on artifact caching.** If the battery is green but reruns
   are still slow or too hand-wired, claim `m80-16hx7.2`. If smoke's current
   rebuild-on-stale-manifest path is sufficient, leave `.2` open as
   operational polish rather than blocking v0.1.
3. **Close cheap correctness/documentation P1 drift.** The next best no-KVM
   wins are `m80-p4i52.1` and `m80-ij49d.{1,2,3}`: publish safety plus README
   claims that disagree with code. These align with the "docs must always be
   correct" rule and should be done before broad refactors.
4. **Harden failure visibility and cleanup.** Pull `m80-wok08.3`,
   `m80-243wj.17`, and adjacent failure-path cleanup beads forward if the
   real-KVM reports keep showing leaked processes, swallowed thread panics, or
   missing preserved-run-dir evidence.
5. **Improve production observability.** After the battery is trustworthy,
   work `m80-9jfaz.2`, `m80-mrjqs.3`, and `m80-mrjqs.4` so future CI and
   operator failures are diagnosable from structured events rather than local
   archaeology.
6. **Defer heavy refactors until the bar is green.** `m80-ezs0x.1` /
   `m80-ezs0x.2` are real problems, but launch/exec refactors touch the most
   failure-sensitive surface. Do them after the privileged battery gives a
   reliable safety net.
7. **Perf stays secondary.** `m80-jp6ik` remains valuable, but it is not the
   v0.1 gate. Resume it after smoke + full ignored battery are green.

## Deferred (not v0.1, no calendar)

Not abandoned — just not blocking the v0.1 bar.

- **`m80-ekbk` — perf bench coverage.** 11 sub-epics, 110+ children.
  Important for the multi-tenant scale story but bench targets aren't ship
  gates; once smoke + battery are green, perf hardening continues against
  measured baselines.
- **`m80-g0v8` L11 (malicious-runner) and L12 (malicious-guestd).** Defense-
  in-depth security verification. Real value but the existing isolation
  claim from FC + the jailer + m80's preflight is sufficient for v0.1; the
  adversarial battery hardens the claim, doesn't establish it.
- **Completed top-of-stack epics (`m80-o4z82`, `m80-2ggw`).** These no longer
  drive active sequencing. Reopen only if a new regression or release-proof
  gap points back to them.
- **`m80-3xwa` — parity with FC defaults beyond what the bar already
  exercises.** Hardening surface that the L11 work would cover anyway.
- **Future direction items in `docs/future-directions/`.** Multi-tenant pool
  conveyor-belt architecture, snapshot diffs, CPU template support — out of
  v0.1 scope by design.

## Why this doc exists

Without it, an agent or new contributor has to infer priority from
`br ready`, recent commits, and conversation history. That works for the
current session but evaporates on context switch. This doc collapses the
inference into a single editable record. When priorities shift — a new
ship gate appears, a deferred epic gets pulled forward, the consumer
profile changes — update here in the same diff that changes the work.

## Where this fits

- `AGENTS.md` — workspace doctrine + scope boundary (stable; rarely changes)
- `README.md` — user-facing capability surface (stable until public API moves)
- `TARGET.md` (this file) — current focus + ship bar (volatile; expect edits)
- `.beads/` — granular work + per-leaf priority (live planning)
- `docs/behaviors/<area>/<topic>.md` — what individual closed beads pin
- `docs/adapter-boundary.md`, `docs/positioning.md` — what is and isn't m80
