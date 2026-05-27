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

1. **`m80-ezs0x.{1,2}` — file-size refactors after coverage/observability.**
   These are real problems, but `launch.rs` and exec/lifecycle refactors touch
   the most failure-sensitive paths and require real-KVM smoke evidence.

Systemd-first launch work is closed under `m80-9wm35`. Current behavior is
systemd-first on hosts where preflight can create a supported system transient
unit, with `m80-jailer-harden` retained as the feature-gated fallback for hosts
without supported systemd. Phase 2 final-exec-site investigation is closed
under `m80-92eor`: ADR 0012 chooses upstream Path A for a future
`--final-exec-hardening` official-jailer flag, while ADR 0011 says m80 accepts
the documented residual rather than forking Firecracker or building an m80-owned
launcher if upstream rejects or stalls. This does not block the next release;
release notes must keep the claim bounded to inherited launch-path hardening,
official-jailer setup/uid-gid exec, and Firecracker-owned VMM seccomp.

Closed proof chain: `m80-16hx7.9`, `m80-16hx7.10`, and `m80-16hx7.11`
produced the nested-KVM public-install smoke artifact for `v0.2.20`. The
current public installer proof is `v0.2.22`: it keeps the flat
`/opt/m80/{bin,artifacts}` hardlink layout, preserves the versioned rollback
record, passed no-auth public release receipt verification, and installed cleanly
on vulcan with plenum/torpor consumers Running.
`m80-16hx7.1`, `.3`, and `.4` are now closed as of 2026-05-25: runner setup is
documented/idempotent, ignored tests have enforced structured reasons, no-KVM
and privileged selector proofs are green, and the pre-test reaper has seeded
real-host residue coverage. `m80-16hx7.6` now has a documented/validated JSON
report schema for the wrapper output. `m80-16hx7.5` now adds per-test
before/after leak diffs, reports newly leaked host state as `leak-check`, and
has a live wrapper proof against the helper package's ignored reaper test.
`m80-16hx7.7` and `m80-s3r28.2` are closed as of 2026-05-26: GitHub
Privileged E2E run
`https://github.com/moradology/m80/actions/runs/26443657120` on commit
`516ebce550ed6358a4f292f1923d9bd96c250277` passed `scripts/smoke.sh`, the full
ignored privileged battery, report validation, and artifact upload. Validated
report:
`/tank/tmp/m80-e2e-gh-run-26443657120/m80-e2e-privileged-26443657120-1/m80-e2e-report.json`
with `pass=111 fail=0 skip=33 total=144`. Current runner proof:
`m80-l1-runner` is registered as a repo self-hosted runner with labels
`self-hosted,Linux,X64,kvm`; Privileged E2E run
`https://github.com/moradology/m80/actions/runs/26450809508` on commit
`185d88e9af22a3e6ec543ef6dccac3f540e98946` passed anonymous checkout,
substrate checks, `scripts/smoke.sh`, external-network ignored tests, report
validation, diagnostics capture, and artifact upload. Validated report:
`/tank/tmp/m80-e2e-gh-run-26450809508/m80-e2e-privileged-26450809508-1/m80-e2e-report.json`
with `pass=117 fail=0 skip=28 total=145`.
`m80-p4i52.1` and `m80-ij49d.{1,2,3}` are closed as of 2026-05-26 by
`dcbea1b0`: the workspace is non-publishable, manifest metadata proves all 24
packages have publishing disabled, and the firecracker/cgroup/CLI README drift
found in those leaves is corrected with CLI parser regressions.
`m80-16hx7` is closed as of 2026-05-26: runner setup, taxonomy/selective
invocation, pre-test reaping, leak checks, JSON reporting, local-dev docs, and
self-hosted CI are in place. `m80-16hx7.2` is intentionally deferred as artifact
cache polish because the harness is usable without it; do not treat that cache
as part of the v0.1 ship bar unless rerun friction becomes material.
`m80-vpw49.1` is closed as of 2026-05-26: no-KVM tests now pin restore-probe
request-id handling, restore-probe retry/timeout behavior, join-netns PID-1
network tokens, cgroup probe error mapping, and snapshot path staging.
`m80-vpw49.2` is closed as of 2026-05-26: no-root tests now pin inherited-fd
closure while preserving stdio, and existing root/plan/recover tests cover the
capability and m80-jailer absorbed sub-gaps.
`m80-vpw49.6` is closed as of 2026-05-26: storage tests now pin TemplateLock
timeout and non-`AlreadyExists` error branches, existing metadata and root/loop
scratch tests prove stale sidecar and symlink-image behavior, and absorbed
snapshot/image leaves now cover malformed snapshot bytes, parent traversal
rejection, source-rootfs sha256 half-None cases, Ubuntu dry-run sha256 labeling,
and `KernelKind::Stock` manifest round-trip.
`m80-mrjqs.4` is closed as already satisfied by current source: the client
exposes `put_logger`, launch configures Firecracker's native logger before
metrics, `docs/behaviors/diagnostics/fc-native-logger.md` records the contract,
and focused logger tests pass.
`m80-9jfaz.2` is closed as of 2026-05-26: launch/restore/exec/capture/stop and
warm-lease exec entrypoints open `vm_id` spans, per-VM teardown warning/error
events carry structured `vm_id`, `MaterializedJail` and cgroup `Subtree` store
VM ids for Drop logging, and the contract is pinned in
`docs/behaviors/observability/vm-id-correlation.md`.
`m80-mrjqs.3` is closed as of 2026-05-26 by `2d12982d`: the production metrics
surface is ungated, `m80-firecracker` records process-local launches, phase
failures, finite `FcError` variants, vsock disconnects, and idle timeouts,
`m80 metrics` renders Prometheus text or a node-exporter textfile and includes
active warm-owner slot metrics when present, and
`docs/behaviors/observability/production-metrics-surface.md` records the
process-local counter boundary.
`m80-vpw49.8` is closed as of 2026-05-26: L1 real-KVM proof with
`M80_RUN_EXTERNAL_NETWORK_E2E=1` passed all outbound egress ignored tests,
including external DNS/HTTP, HTTP by IP, ICMP default-deny, peer-guest isolation,
private Firecracker netns, and post-drop CAP_NET_ADMIN behavior. Proof artifacts:
`/tank/tmp/m80-vpw49-8-proof/m80-vpw49-8-egress.log`,
`/tank/tmp/m80-vpw49-8-proof/m80-cap-net-admin-drop.log`, and
`/tank/tmp/m80-vpw49-8-proof/m80-cap-drop-smoke.log`. The full GitHub
Privileged E2E proof above also ran with `external_network=true`.
`m80-25mdp` moves the privileged workflow toward untrusted-contribution safety:
manual dispatch can target a unique runner label, and
`scripts/register-l1-github-runner.sh` registers an existing L1 as a one-job
ephemeral GitHub runner with that label. This is not full arbitrary-fork
automation yet; runner creation remains a trusted operator/control-plane step.
As of the v0.2.25 follow-up, `.github/workflows/e2e-privileged.yml` no longer
has tag, schedule, or pull-request triggers and no longer falls back to the
durable `kvm` label; every run is explicit `workflow_dispatch` to a broker
issued `m80-e2e-*` label plus the `m80-privileged-e2e` class label.

## Long-run order

This is the working order when someone asks to keep pushing for a long stretch.
Do not fan this into dozens of new leaves unless the report proves independent
root causes.

1. **Keep the privileged battery green.** The self-hosted workflow is now the
   source of truth for kernel-touching confidence. If it fails, first preserve
   and validate the JSON report, then fix or file only report-proven root
   causes. Current proof: `m80-l1-runner` is online as a repo
   `self-hosted,kvm` runner; run `26450809508` is green on
   `185d88e9af22a3e6ec543ef6dccac3f540e98946`, the latest code/workflow commit
   in the E2E wiring line; follow-up proof-recording commit `fc64ba09` passed
   CI run `26455499511`.
   For higher-risk PR proof, prefer the documented disposable L1 path with a
   unique `runner_label` over the durable `kvm` runner.
2. **Treat `m80-f20y0.1` as strategic context, not a code leaf, until the
   external consumer integration has a concrete m80 gap.** The observability
   batch that was gating long-run diagnosis is complete enough for the next
   proof/refactor work.
3. **Harden failure visibility and cleanup if E2E reports regress.** Pull
   `m80-wok08.3`, `m80-243wj.17`, and adjacent failure-path cleanup beads
   forward only when the real-KVM reports show leaked processes, swallowed
   thread panics, or missing preserved-run-dir evidence.
4. **Defer heavy refactors until the coverage lane lands.** `m80-ezs0x.1` /
   `m80-ezs0x.2` are real problems, but launch/exec refactors touch the most
   failure-sensitive surface. Do them after the no-KVM coverage and
   observability lanes give a better safety net, and attach fresh real-KVM
   smoke evidence to the refactor commit.
5. **Perf stays secondary.** `m80-jp6ik` remains valuable, but it is not the
   v0.1 gate. Resume it after smoke + full ignored battery are green.

## Deferred (not v0.1, no calendar)

Not abandoned — just not blocking the v0.1 bar.

- **`m80-ekbk` — perf bench coverage.** 11 sub-epics, 110+ children.
  Important for the multi-tenant scale story but bench targets aren't ship
  gates; once smoke + battery are green, perf hardening continues against
  measured baselines.
- **`m80-16hx7.2` — artifact build/cache.** Useful if repeated privileged
  reruns become too slow, but explicitly not part of the v0.1 bar now that the
  runner/reporting path is proven without it.
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
