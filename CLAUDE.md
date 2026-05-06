# m80 — generic Firecracker VM sandboxing

A Rust workspace that lifts Firecracker microVM orchestration out of `predecessor` and ships it as a reusable library + CLI + image-build tool. m80 is **generic sandboxing**: boot a kernel + rootfs, optionally give it a workspace and outbound NAT, run a command, return stdout/stderr/exit, tear down without leaks. Useful for agents — but not coupled to them.

## Read first

1. `README.md` (dossier intro)
2. `crates/<name>/README.md` for the crate you're touching — every crate's README **is** its contract; treat it as authoritative
3. The active bead and its acceptance criteria
4. `00-verdict.md` only — reach for the rest of the dossier when you need archaeology

## Scope boundary — non-negotiable

m80 captures **VM mechanics only**. The following live in a future `m80-adapter`, NOT here:

- Tool catalog (`bash`/`exec`/`read_file`/...) and tool-registry validation
- Semantic identifiers as carriers of meaning: `tool_call_id`, `correlation_id`, `idempotency_key`, `workspace_id`. m80's wire carries an opaque `request_id` at most
- `EffectClass::ReadOnly|Mutating` and writeback-authority gating; m80 change-extraction is opt-in by caller request
- Idempotency / duplicate-delivery contract; commit-authority deadline gating; authority-lease state machine
- Stage-H semantic events (`sandbox_exec_*`, `runtime_reset_triggered`). m80 emits VM-lifecycle events only
- `WorkspacePolicy { read_only, allowed_tools }` enforcement
- Kubernetes pod contract (hostPath, ready-marker file, drain timing)

Test: if a behavior is "what the system does for the agent", it's not m80's. If it's "what the VM and host do to make a sandboxed exec possible", it is.

## Workspace shape

16 crates under `crates/`, each black-boxed:

- **Foundation (9):** `m80-proto`, `m80-image-manifest`, `m80-firecracker-client`, `m80-vsock`, `m80-jailer`, `m80-cgroup`, `m80-storage`, `m80-preflight`, `m80-net-mode`. Privilege is acquired by the m80 process at startup (run as root, `setcap` the binary, or run inside a privileged container) and verified by `m80-preflight`; there is no per-call privilege shim.
- **Networking (1):** `m80-net-outbound` — OutboundNat policy/iptables/DNS/cleanup (~3500 LOC; dossier's #1 risk)
- **Deferred-but-reserved (2):** `m80-snapshot` (schemas active v0.1, execution v0.2), `m80-observability` (empty v0.1, populated v0.2)
- **Orchestration (1):** `m80-firecracker` — composes foundation crates; owns the lifecycle state machine and run-root layout
- **Binaries (3):** `m80-image-build`, `m80-guestd` (cross-compiled, in-VM), `m80-cli` (the `m80` binary)

Cardinal rules:
- No `m80-common` / `m80-shared` / `m80-utils` junk drawers. Cross-cutting types live in the crate that owns the producing domain.
- A change to a crate's public surface updates its README in the same diff. Drift is a bug.
- Workspace deps in the root `Cargo.toml`; members use `xxx.workspace = true`. No direct version pins in member manifests.

## Planning state — beads

Live planning is in `.beads/` (prefix `m80-`, separate from predecessor). Today: 246 leaf behavior captures under 18 L1 epics (~332 beads total). Each leaf is a present-tense fact about the working predecessor implementation; closing it requires **both** a doc at `docs/behaviors/<area>/<topic>.md` AND a test at `crates/<crate>/tests/<area>/<topic>.rs`.

Workflow: `bv --robot-triage` → `br update <id> --claim` → do work → `br update <id> --notes "doc=…; test=…"` → `br close <id> --reason "captured"`. Stage `.beads/issues.jsonl` alongside code on commit (auto-flush is on by default).

Bead authoring tooling lives under `specs/`: `01-skeleton.sh`, `02-author-leaves.py`, `02b-backfill-acceptance.py`, `03-validate.sh`, `04-deps.sh` are re-runnable; `leaves-*.md` is the human-reviewable spec source.

## Rust expectations

- `unsafe` is forbidden (workspace lint). Safe wrapper crates at any FFI boundary.
- `thiserror` for library errors; `anyhow` only at binary edges.
- Synchronous APIs unless the boundary requires async. No `tokio` in foundation crates.
- RAII guards bind to named locals; don't rely on tail-expression temporary lifetimes.
- Tests with the code: unit for semantics, integration for boundaries.
- Public APIs narrow; invariants documented in the crate README.

## File size

- **Avoid Rust files larger than 500 lines.** When a file approaches that, split by domain (types vs framing vs serde helpers, etc.) into a module tree.
- **Hard limit: 1000 lines.** A file that crosses 1000 is a bug; refactor before merging.
- This rule applies to `lib.rs`, `main.rs`, and any submodule. Tests living next to code are included in the count — split them out into a `tests/` mod tree if the file is overflowing.

## When making changes

- Smallest surface that satisfies the bead.
- Touch a crate's public surface → update that crate's README's "Public surface" / "Black-box contract" / "Non-goals" sections in the same diff.
- Touch a non-trivial invariant → write a regression test that pins it.
- Don't widen scope because a related cleanup looks tempting.

## When debugging — diagnostics before hypotheses

When a host-side operation fails and the host can't directly observe the guest (or vice versa across any opaque boundary), the very first move is to **make the other side's stderr visible**. Form hypotheses *after* you have logs from both sides, not before.

Concretely:
- A launch hangs or times out → before guessing at vsock races, kernel issues, or muxer state, ensure m80-guestd's stderr reaches a place you can read (`StandardError=journal+console` on the systemd unit, or direct console output for PID-1 mode). One timestamped log line from the guest typically pinpoints the issue in seconds; without it, every hypothesis is a guess.
- A symptom that looks like it's at layer N may be at layer N±1 or N±2. Visibility cuts across layers; speculation doesn't.
- Lessons from real incidents: the ubuntu/idle 40 % flake looked like a vsock-muxer race in the host's CONNECT/RST polling. After ~hour of investigation (replacing polling with inverted readiness, chasing benign virtio-mmio kernel warnings, fighting bash background ghosts), the actual cause was systemd boot ordering — `m80-guestd.service` had `WantedBy=multi-user.target`, `multi-user.target` was bimodal (~1 s on success vs full 90 s on `network-wait-online` timeout), and m80-guestd starting at T=91 s missed the host's READY_TIMEOUT. The fix was three lines in the service unit (`DefaultDependencies=no`, `After=local-fs.target workspace.mount`, `WantedBy=basic.target`). Visible guest stderr would have surfaced "started at T=91 s" immediately and saved the hour.

Corollary: when debugging in a tight loop, **make the diagnostic visibility change first**, then iterate. The instinct to "try one more fix" before adding logging usually costs more than the logging would.

## What we don't want

m80 is a v0.x internal crate set with a closed call graph (we own every consumer). Defensive, future-proofing, and "be tolerant" patterns aimed at unknown third-party callers waste tokens, hide real failures, and rot the moment the assumption shifts.

- **No "be tolerant" wrappers on internal data.** No case-insensitive / whitespace-tolerant / leniency shims when both producer and consumer are m80 crates. If the comparison is `==`, write `==`.
- **Serde inputs fail closed.** Internal config, manifest, persisted state, and wire DTOs that derive `Deserialize` use `#[serde(deny_unknown_fields)]`. The only exception is a documented partial probe such as `schema_version` / `version` pre-parse, where unknown fields must be ignored long enough to return the right version error.
- **No `#[non_exhaustive]` on internal-only enums** in pre-1.0 crates. A compile error in our own `match`es when we add a variant is the signal we want.
- **No hand-rolled canonicalization, normalization, or stable-ordering helpers** unless something *concretely* consumes the canonical form (archival hashing, signature verification, on-the-wire diff). "Just in case" is not a consumer.
- **No catch-and-rewrap of I/O errors** that already say what failed. If you do reclassify (e.g., `NotFound` → a domain variant), do it uniformly across the surface — asymmetric reclassification hides which call site lost the file.
- **No silent recovery** — no auto-creating parent dirs, no auto-padding short inputs, no default-fallback-on-error. Surface the failure; let the caller decide.
- **No premature abstraction**. Three near-identical lines beat a generic helper. A `Vec::push` loop beats a builder pattern.
- **No bundling failure scenarios into one test fn.** Each scenario is its own `#[test]`; first failure must not mask the rest.
- **No tamper-and-restore disk patterns** when clone-and-mutate-the-struct gets the same coverage without coupling test order to filesystem state.

If you find yourself writing one of these because "what if someone…", stop. We are the someone. Change the code, not the assumption.

## Dossier (`00-verdict.md` … `11-loc-and-surface-budget.md`)

Historical analysis from the predecessor read. Useful for **why** decisions were made (coupling, network internals, LOC budget). Not normative — the crate READMEs and the beads are. Reach for the dossier when you need to recover *intent*.
