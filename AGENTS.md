# m80 — generic Firecracker VM sandboxing

A Rust workspace that ships Firecracker microVM orchestration as a reusable library + CLI + image-build tool. m80 is **generic sandboxing**: boot a kernel + rootfs, optionally give it a workspace and outbound NAT, run a command, return stdout/stderr/exit, tear down without leaks. Useful for agents — but not coupled to them.

## Read first

1. `TARGET.md` — current focus, ship bar, top-of-stack epics. Read **before** picking up new work.
2. `README.md` — user-facing capability surface and crate catalog.
3. `crates/<name>/README.md` for the crate you're touching — every crate's README **is** its contract.
4. The active bead and its acceptance criteria.
5. `docs/adapter-boundary.md` and `docs/positioning.md` when designing public surface.

## Scope boundary — non-negotiable

m80 captures **VM mechanics only**. The following live in an external adapter consumer, NOT here:

- Tool catalog (`bash`/`exec`/`read_file`/...) and tool-registry validation
- Semantic identifiers as carriers of meaning: `tool_call_id`, `correlation_id`, `idempotency_key`, `workspace_id`. m80's wire carries an opaque `request_id` at most
- `EffectClass::ReadOnly|Mutating` and writeback-authority gating; m80 change-extraction is opt-in by caller request
- Idempotency / duplicate-delivery contract; commit-authority deadline gating; authority-lease state machine
- Stage-H semantic events (`sandbox_exec_*`, `runtime_reset_triggered`). m80 emits VM-lifecycle events only
- `WorkspacePolicy { read_only, allowed_tools }` enforcement
- Kubernetes pod contract (hostPath, ready-marker file, drain timing)

Test: if a behavior is "what the system does for the agent", it's not m80's. If it's "what the VM and host do to make a sandboxed exec possible", it is.

## Workspace cardinal rules

- No `m80-common` / `m80-shared` / `m80-utils` junk drawers. Cross-cutting types live in the crate that owns the producing domain.
- Workspace deps in the root `Cargo.toml`; members use `xxx.workspace = true`. No version pins in member manifests.
- A change to a crate's public surface updates that crate's README in the same diff. Drift is a bug.
- Each closed bead leaf needs **both** a doc at `docs/behaviors/<area>/<topic>.md` AND a test at `crates/<crate>/tests/<area>/<topic>.rs`.
- **Measurement-shaped beads carry the `requires-verified-close` label.** Their close requires a `verified: <artifact-path> @ <commit>` reason citing the file that contains the real-substrate measurement. Agents may scaffold; only a human (or an agent with explicit operator confirmation of a real-substrate run) closes. Parent epics inherit the label transitively. See [decision 0002](docs/decisions/0002-bead-closure-scaffolded-vs-verified.md). Motivated by the m80-ekbk false close (2026-05-12).
- **Measurement-shaped beads name an observable, not an action.** Acceptance must read "`<file/artifact>` contains `<named numeric>` <=/>=/=/within `<bound>` on `<substrate>`" or "`<command>` on `<substrate>` exits 0 and emits `<observable>`". NOT "harness runs", "compute layer green", or "tests pass". The observable is what the future verifier checks; the action is what the agent did. See [`.beads/bead-template-measurement.md`](.beads/bead-template-measurement.md). Motivated by m80-ekbk false close (2026-05-12).

## Rust expectations

- `unsafe` is forbidden (workspace lint). Safe wrapper crates at any FFI boundary.
- `thiserror` for library errors; `anyhow` only at binary edges.
- Synchronous APIs unless the boundary requires async. No `tokio` in foundation crates.
- RAII guards bind to named locals; tail-expression temporaries drop too early (the `LeaseGuard` footgun).

## File size

Avoid Rust files larger than 500 lines. **Hard limit: 1000 lines** — files crossing this are a bug to refactor before merging. Tests next to code count toward the limit; split into a `tests/` mod tree if overflowing.

## When making changes

- Smallest surface that satisfies the bead.
- Touch a non-trivial invariant → write a regression test that pins it.
- Touch a kernel-facing primitive (mount/ns/caps/signal/seccomp/cgroup/sudo
  wrapping) → PR must include a real-KVM smoke paste. See
  [`AGENTS.md`](AGENTS.md) → "Kernel-touching diffs require smoke evidence".

## Kernel-touching diffs require smoke evidence

If a diff touches any of:

- mount flags / `MsFlags` / `mount(2)` call sites
- namespace flags / `CloneFlags` / `unshare(2)` / `setns(2)`
- capability sets (`caps::Capability`, `prctl(PR_CAPBSET_DROP)`)
- signal masks / `sigprocmask` / `SigSet`
- seccomp filters / BPF programs
- cgroup v2 controller writes (`cgroup.subtree_control`, `cpu.max`,
  `memory.max`, etc.)
- `sudo` invocation wrappers in `scripts/`
- `scripts/smoke.sh`, `scripts/bench-*.sh`, `scripts/test-bench-harness.sh`

...then the PR description **must** include a paste of a green run of the
relevant smoke (`./scripts/smoke.sh` for jailer/firecracker primitives;
`./scripts/bench-cold-launch.sh N=10 --dry-run` is NOT sufficient; a real
N>=10 run is) from a host with `/dev/kvm` accessible.

This rule exists because on 2026-05-12 three regressions in this exact class
(`MS_BIND` removal, `m80_invoke`-after-`sudo`, stale image dir default) all
passed `cargo test`, shellcheck-class linters, and mock "e2e" coverage. Only
a real-KVM smoke surfaced them. See
[`docs/postmortems/2026-05-12-ms-bind-and-sudo-escape.md`](docs/postmortems/2026-05-12-ms-bind-and-sudo-escape.md).

For audit-sweep work specifically: a sweep that touches the categories above
is **not** an eligible sweep; it is a kernel-behavior change in sweep clothing.
Split it out and ship as a normal diff with the smoke.

Mock-based "e2e" tests do not satisfy this rule. The mock was the proximate
cause of the false-green window.

## Audit-sweep eligibility

Audit-sweep methodology (an agent making mechanical, doctrine-driven changes
across many files) is net-positive on statically-verifiable code and
net-negative on kernel-behavior-determining code. Sweep PRs touching the
**sweep-ineligible** patterns below must split those hunks out and ship them as
single-purpose diffs with full smoke evidence per the "Kernel-touching diffs
require smoke evidence" rule.

**Sweep-ineligible patterns** (non-exhaustive; see
[decision 0003](docs/decisions/0003-audit-sweep-eligibility.md)):

- `nix::mount::*`, `nix::sched::CloneFlags`, `nix::sys::signal::SigSet`
- `caps::Capability`, capability-set construction
- seccomp filter / BPF construction
- cgroup v2 controller writes
- `mount(2)`, `umount2(2)`, `pivot_root(2)`, `chroot(2)`
- `clone(2)`, `unshare(2)`, `setns(2)`
- `sudo` wrappers in `scripts/`
- `scripts/smoke.sh`, `scripts/bench-*.sh`
- `crates/m80-jailer/src/plan.rs`, `crates/m80-jailer-harden/src/`,
  `crates/m80-firecracker/src/lifecycle/exec.rs`

Sweep agents must read this list before claiming a sweep bead. If the sweep
would touch any ineligible pattern, the agent must report the candidates
without modifying them, and a human must drive the single-purpose diff.

## When debugging — diagnostics before hypotheses

When a host-side operation fails and the host can't directly observe the guest (or vice versa across any opaque boundary), the very first move is to **make the other side's stderr visible**. Form hypotheses *after* you have logs from both sides, not before. A symptom that looks like layer N may be at layer N±1 or N±2 — visibility cuts across layers; speculation doesn't.

Corollary: when debugging in a tight loop, make the diagnostic visibility change first, then iterate. The instinct to "try one more fix" before adding logging usually costs more than the logging would.

For worked examples of this discipline (and the cost of skipping it), read `docs/postmortems/`.

## What we don't want

m80 is a v0.x internal crate set with a closed call graph — we own every consumer. Defensive, future-proofing, and "be tolerant" patterns waste tokens, hide real failures, and rot the moment the assumption shifts.

- **No "be tolerant" wrappers on internal data.** If the comparison is `==`, write `==`. No case-insensitive / whitespace-tolerant shims when both producer and consumer are m80.
- **Serde inputs fail closed.** Internal config, manifest, persisted state, and wire DTOs use `#[serde(deny_unknown_fields)]`. Exception: a documented partial probe (`schema_version` / `version` pre-parse) where unknown fields must be ignored long enough to return the right version error.
- **No `#[non_exhaustive]` on internal-only enums** in pre-1.0 crates. A compile error in our own `match`es when we add a variant is the signal we want.
- **No typed error collapse at public boundaries.** Errors carry finite typed variants plus detail, not `Other(String)` / `Generic` with prose. CLI exit-code mapping stays exhaustive over `FcError`.
- **No hand-rolled canonicalization, normalization, or stable-ordering helpers** unless something concretely consumes the canonical form (archival hashing, signature verification, on-the-wire diff). "Just in case" is not a consumer.
- **No catch-and-rewrap of I/O errors** that already say what failed. If you do reclassify (e.g., `NotFound` → a domain variant), do it uniformly — asymmetric reclassification hides which call site lost the file.
- **No silent recovery** — no auto-creating parent dirs, no default-fallback-on-error. Surface the failure; let the caller decide.
- **No premature abstraction.** Three near-identical lines beat a generic helper. A `Vec::push` loop beats a builder pattern.
- **No bundling failure scenarios into one test fn.** Each scenario is its own `#[test]`; first failure must not mask the rest.
- **No tamper-and-restore disk patterns** when clone-and-mutate-the-struct gets equal coverage without coupling test order to filesystem state.
- Audit-sweep agents must follow the sweep-ineligible classifier in
  `AGENTS.md` before modifying kernel, cgroup, seccomp, capability, or `sudo`
  wrapper code.

If you're writing one of these because "what if someone…", stop. We are the someone. Change the code, not the assumption.

## Where to find things

- **Current focus + ship bar:** `TARGET.md` (volatile).
- **Crate-level contracts:** `crates/<name>/README.md`.
- **Behavior captures (bead-paired):** `docs/behaviors/<area>/<topic>.md`.
- **Incident postmortems:** `docs/postmortems/<date>-<slug>.md`. Read when a bug resembles a past one.
- **Architecture decision records:** `docs/decisions/<NNNN>-<slug>.md`. Read before re-proposing an alternative — most "obvious" approaches were already evaluated and rejected.
- **Adapter boundary + promotion bar:** `docs/adapter-boundary.md`.
- **Positioning vs smolvm/kata:** `docs/positioning.md`.
- **Future direction:** `docs/future-directions/`.
- **Performance data:** `docs/perf/cold-launch.md`.
- **Live planning:** `.beads/` (CLI: `br ready`, `br stats`, `br show <id>`).
