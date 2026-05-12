# m80 — Current Target

This file is the single source of truth for **what m80 is working toward right
now**. It complements (does not replace) the scope boundary in `CLAUDE.md`,
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

1. **`m80-o4z82` — Real-KVM regression cluster** (open until privileged
   battery confirms). The diagnostic + structural fix is in
   (`ConfigError::VmIdPathBudgetExceeded` admission check, vm_id helpers
   trimmed, fake-firecracker now writes pid, log assertion scoped to
   diagnostics phase). Verifying the 6 affected tests pass on the
   privileged runner is the close gate.
2. **`m80-16hx7` — E2E operationalization.** Stable privileged runner +
   pre-test reaper + ignored-test taxonomy. Without these, the privileged
   battery is "manually-provable" not "routinely-enforced", which means
   gate (2) above is hard to verify in practice. Children `.1` (runner),
   `.3` (taxonomy), `.4` (reaper) are the unblocking subset; `.2` (cache)
   and `.6` (reporting) and `.7` (CI) can lag.
3. **`m80-2ggw` — FC best-practices alignment, P1 children only:**
   - `m80-2ggw.1` host preflight enforcement (KSM / SMT / swap / nested-virt
     checks) — multi-tenant safety claim depends on this.
   - `m80-2ggw.4` snapshot correctness (FC version validation on restore;
     post-restore CSPRNG re-seed in m80-guestd) — warm-pool integrity.
   - `m80-2ggw.6` IMDS link-local block regression pin — already-present
     security boundary, just needs the test to pin it.

The P2/P3 children inside `m80-2ggw` (FC API surface like put_logger /
put_metrics, kernel feature evaluation, operator runbook) wait until the
P1 children land and the ship bar is green.

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
- **`m80-2ggw` P3 children.** Operator runbook docs, runtime feature
  evaluation (PVH, hugepages, virtio-rng). Documentation polish + perf
  options that don't gate the bar.
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

- `CLAUDE.md` — workspace doctrine + scope boundary (stable; rarely changes)
- `README.md` — user-facing capability surface (stable until public API moves)
- `TARGET.md` (this file) — current focus + ship bar (volatile; expect edits)
- `.beads/` — granular work + per-leaf priority (live planning)
- `docs/behaviors/<area>/<topic>.md` — what individual closed beads pin
- `docs/adapter-boundary.md`, `docs/positioning.md` — what is and isn't m80
