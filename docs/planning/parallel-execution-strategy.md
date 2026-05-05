# Parallel-execution strategy for the perf roadmap

**Date:** 2026-05-05
**Scope:** the four perf-roadmap epics (`m80-f2zc`, `m80-ci9i`, `m80-rrp.3`, `m80-qokt.2`) — 32 leaves total.
**Goal:** maximum parallelism via sonnet sub-agents in isolated git worktrees, with file-territory boundaries that minimize cross-agent merge conflicts.

## The four lanes

Each epic = one lane. Within a lane, leaves are sequential (DAG'd via `blocks:` deps). Across lanes, work runs in parallel where file territories don't overlap.

| Lane | Epic | Scope summary |
|---|---|---|
| **A** | `m80-f2zc` storage pivot | 9 leaves; biggest crate footprint |
| **B** | `m80-ci9i` stripped kernel | 6 leaves; concentrated in image-build + a small piece of m80-firecracker |
| **C** | `m80-rrp.3` snapshot/restore | 11 leaves; largest by effort; cross-cuts firecracker-client + snapshot + firecracker |
| **D** | `m80-qokt.2` persistent VM | 6 leaves; concentrated in m80-firecracker lifecycle + tests + a slice of m80-proto/guestd |

## File-territory map

Crates / files grouped by primary owner. Contested files are flagged.

| Path | Owner | Contention notes |
|---|---|---|
| `crates/m80-storage/**` | A | clean |
| `crates/m80-firecracker-client/**` | C | clean |
| `crates/m80-snapshot/**` | C | clean |
| `crates/m80-image-build/kernel-builder/**` | B | NEW; clean |
| `crates/m80-image-build/kernels/**` | B | NEW; clean |
| `crates/m80-image-build/src/pipeline.rs` | B (kernel build path), A (CONFIG_OVERLAY_FS verify) | **A.5 piggy-backs B.2's commit** — single agent owns pipeline.rs additions |
| `crates/m80-image-manifest/src/lib.rs` | B (schema bump 2→3 for `kernel.kind`) | Single owner |
| `crates/m80-firecracker/src/launch.rs` | A (phase_11 drive PUT), B (boot_args_for cmdline), C (Sandbox::launch_from_snapshot) | **launch.rs is the contested file**. Strategy: A goes first; B rebases; C rebases. Merges are small (different fns). |
| `crates/m80-firecracker/src/lifecycle.rs` | D (mut-self), C (Sandbox::capture_running) | Sequence D first (mut-self refactor), then C (capture method) |
| `crates/m80-firecracker/src/types.rs` | D (Poisoned variant on RunningSandbox) | Single owner |
| `crates/m80-firecracker/tests/*.rs` | D (persistent_state.rs, cancellation.rs), C (snapshot integration) | Different test files; clean |
| `crates/m80-guestd/src/pid_one.rs` | A (overlayfs + pivot_root) | Single owner |
| `crates/m80-guestd/src/main.rs` | A (overlay+pivot wiring), C (vsock redial — m80-rrp.3.9) | Sequence A → C |
| `crates/m80-guestd/src/connection.rs` | D (Cancel handler — m80-qokt.2.4) | Single owner *if* m80-5vha doesn't land first |
| `crates/m80-vsock/src/lib.rs` | C (redial helper) | Single owner |
| `crates/m80-proto/src/types.rs` | D (Cancel envelope — qokt.2.4); cross-ref m80-5vha | First-to-land owns; second consumes |
| `crates/m80-cli/src/cmds.rs` | D (idle_timeout, &mut self consumers), C (--from-snapshot subcommand) | Different subcommands; small merge |
| `docs/design/storage-overlay.md` | A.1 | NEW; clean |
| `docs/design/stripped-kernel.md` | B.1 | NEW; clean |
| `docs/design/snapshot-restore.md` | C.11 | NEW; clean |
| `docs/design/persistent-vm.md` | D.1 | NEW; clean |
| `docs/exploration/firecracker-vsock-snapshot.md` | C.10 (research) | NEW; clean |
| `docs/perf/cold-launch.md` | All BENCH leaves | End-stage; sequential; one final commit |
| `CHANGELOG.md` | All | End-stage; one final commit |

**Five contested files:** `launch.rs`, `lifecycle.rs`, `pipeline.rs`, `main.rs` (guestd), `cmds.rs` (cli). Each has a single primary owner per wave; rebases on merge are small (different functions/regions).

## Wave plan

Five waves. Within each wave, all listed sonnets run in parallel in their own git worktrees via the Agent tool's `isolation: "worktree"` flag. Between waves, I merge the worktrees back to main, resolve any conflicts, then dispatch the next wave.

### Wave 0 — Designs + research (4 parallel)

All write to fresh files in `docs/design/` or `docs/exploration/`. **Zero file overlap.**

| Sonnet | Lane | Bead | Output file |
|---|---|---|---|
| W0-A | A | `m80-f2zc.1` | `docs/design/storage-overlay.md` |
| W0-B | B | `m80-ci9i.1` | `docs/design/stripped-kernel.md` |
| W0-C | C | `m80-rrp.3.10` (research) | `docs/exploration/firecracker-vsock-snapshot.md` |
| W0-D | D | `m80-qokt.2.1` | `docs/design/persistent-vm.md` |

Each sonnet reads existing planning docs + runs `br show <leaf>`, then synthesizes the design with the locked-in decisions. ~half-day each.

**Integration:** I merge all 4 worktrees in any order (they don't touch each other). Close `.1` leaves on each epic.

### Wave 1 — IMPL on isolated crates (4 parallel)

After Wave 0 designs land. Each sonnet owns one CRATE.

| Sonnet | Lane | Bead | Files |
|---|---|---|---|
| W1-A | A | `m80-f2zc.2` (m80-storage IMPL) | `crates/m80-storage/**` |
| W1-B | B | `m80-ci9i.2` (kernel build pipeline + manifest schema bump) | `crates/m80-image-build/kernel-builder/Dockerfile` (NEW), `crates/m80-image-build/src/pipeline.rs`, `crates/m80-image-manifest/src/lib.rs` (schema bump 2→3) |
| W1-C | C | `m80-rrp.3.11` DESIGN → `m80-rrp.3.12` REST methods | `docs/design/snapshot-restore.md`, `crates/m80-firecracker-client/**` |
| W1-D | D | `m80-qokt.2.2` (mut-self refactor) | `crates/m80-firecracker/src/lifecycle.rs`, `crates/m80-cli/src/cmds.rs` (single-line consumer update) |

**File overlap check:** A=m80-storage, B=image-build+manifest, C=firecracker-client, D=lifecycle+cli. Disjoint. ✅

Effort: A=M, B=L, C=S+S, D=S. Wall-clock for the wave gated by B (~1-2 days).

**Integration:** merge in order C → A → D → B (smallest diffs first). Cross-check no test surface broke (each agent runs `cargo test -p <its-crate>`).

### Wave 2 — Cross-crate IMPL (3 parallel + 1 sequenced)

The contested `launch.rs` shows up here for the first time. A and B both want different functions in it; sequence them.

| Sonnet | Lane | Bead | Files | Notes |
|---|---|---|---|---|
| W2-A | A | `m80-f2zc.3` (m80-firecracker phase_11 drive PUT) | `crates/m80-firecracker/src/launch.rs` (phase_11_rest_puts), `crates/m80-firecracker/README.md` (drive layout) | Owns launch.rs in this wave |
| W2-C | C | `m80-rrp.3.13` (m80-snapshot capture/restore execution) | `crates/m80-snapshot/**` | Clean |
| W2-D | D | `m80-qokt.2.3` (sequential-exec tests) | `crates/m80-firecracker/tests/persistent_state.rs` (NEW) | Clean |

**B sequences after A merges** (same file, different fn). W2-B = `m80-ci9i.3` (cmdline trim in `boot_args_for`); kicks off as soon as W2-A's launch.rs lands.

**File overlap check:** A=launch.rs, C=m80-snapshot, D=tests/. Disjoint among the 3 parallel. B serializes on launch.rs after A. ✅

### Wave 3 — Guestd + snapshot orchestrator (3 parallel + 1 sequenced)

| Sonnet | Lane | Bead | Files |
|---|---|---|---|
| W3-A | A | `m80-f2zc.4` (overlayfs + pivot_root in PID-1) | `crates/m80-guestd/src/pid_one.rs`, `crates/m80-guestd/src/main.rs` (wiring) |
| W3-C | C | `m80-rrp.3.4` (existing — `Sandbox::launch_from_snapshot`) + `Sandbox::capture` in lifecycle | `crates/m80-firecracker/src/launch.rs` (NEW launch_from_snapshot fn), `crates/m80-firecracker/src/lifecycle.rs` (NEW capture method) |
| W3-D | D | `m80-qokt.2.4` (cancellation contract) | `crates/m80-proto/src/types.rs` (Cancel envelope), `crates/m80-guestd/src/connection.rs` (handler), `crates/m80-firecracker/tests/cancellation.rs` (NEW) |

**File overlap check:**
- A = pid_one.rs, main.rs (guestd) — clean
- C = launch.rs, lifecycle.rs — but launch.rs has new fn additions only (after Wave 2's A landed phase_11 changes)
- D = proto/types.rs, connection.rs (guestd), new test file — overlaps with A on guestd! ⚠️

**Resolution:** A's guestd changes are in `pid_one.rs` (PID-1 module); D's changes are in `connection.rs` (connection handler). Different files. Within `main.rs`, A wires the new pid_one mode but doesn't touch the connection-handler logic. Verify via diff after both land; clean rebase expected.

W3-B = `m80-ci9i.4` (BENCH) sequences after Wave 2 lands; runs the bench harness against the post-cmdline-trim binary. Probably a few hours.

### Wave 4 — Smokes + idle timeout (4 parallel)

Pre-BENCH gate per epic.

| Sonnet | Lane | Bead | Files |
|---|---|---|---|
| W4-A | A | `m80-f2zc.9` (smoke checkpoint) | `scripts/smoke.sh` (small additions), `docs/perf/cold-launch.md` |
| W4-B | B | `m80-ci9i.6` (smoke checkpoint) | `scripts/smoke.sh` (NEW kernel-kind switch), `docs/perf/cold-launch.md` |
| W4-C | C | `m80-rrp.3.14` (snapshot smoke) + `m80-rrp.3.5` (CLI surface) | `scripts/smoke.sh` (NEW snapshot path), `crates/m80-cli/src/cmds.rs` (--from-snapshot) |
| W4-D | D | `m80-qokt.2.5` (idle timeout IMPL) | `crates/m80-firecracker/src/types.rs` (SandboxConfig::idle_timeout), `crates/m80-firecracker/src/lifecycle.rs` (timeout wiring) |

**File overlap:** A, B, C all touch `scripts/smoke.sh` and `docs/perf/cold-launch.md`. ⚠️

**Resolution:** smoke.sh diffs are additive (new env-var branches). Sequence: A → B → C smoke.sh updates. cold-launch.md gets one final integration update post-Wave 5.

D is clean.

### Wave 5 — BENCH + DOCS (sequential consolidation)

Single agent (or me directly) — runs benches per epic, captures numbers, updates `docs/perf/cold-launch.md`, writes `CHANGELOG.md` Unreleased entries, closes BENCH and DOCS leaves. ~half-day to run all four benches and write up.

## Worktree mechanics

Each wave dispatch uses `Agent({ isolation: "worktree", ... })`. The Agent tool creates a temporary git worktree on a fresh branch off `main`; the sonnet works inside; on completion the tool returns the branch name + path.

I then:
1. `git fetch <worktree-path>` (or use the path directly)
2. `git merge --ff-only <branch>` (or rebase if not fast-forward)
3. Resolve any conflicts (rare per file-territory map; if frequent, the wave plan is wrong, not the agents)
4. Run `cargo test --workspace` + `cargo clippy --workspace --all-targets -- -D warnings`
5. Commit any merge resolution as a discrete commit
6. Move to next wave

If a sonnet's diff doesn't compile or fails tests on its own branch, that's the sonnet's problem to fix before the worktree is cleanly mergeable. I don't merge broken work.

## Per-sonnet prompt template

Each sonnet gets:

1. **Bead ID(s)** to claim and close. Read via `br show`.
2. **Planning doc reference** (`docs/planning/perf-roadmap-extended.md §<N>`).
3. **Acceptance criteria** as listed in the bead.
4. **File-territory whitelist**: only touch files in your territory. If you need to touch a file outside, stop and report — don't blindly extend scope.
5. **Run `cargo test -p <crate>` + clippy before declaring done**.
6. **Update README per CLAUDE.md "touch a public surface → update README in same diff" rule**.
7. **Close beads via `br close`** with notes citing the commit SHA.

Hand-rolled per sonnet (the m80 convention is "no templates" for the prompts themselves; this is shared structure not template content).

## Failure handling

- **Sonnet fails partway**: worktree retained for inspection; I either fix manually or restart with a tighter prompt.
- **Merge conflict**: file-territory map is wrong; revisit before continuing; resolve manually for the immediate landing.
- **Tests fail post-merge**: rollback the merge (`git revert`); investigate; fix in a tight loop with diagnostics-first per CLAUDE.md.
- **Cross-epic dep surfaces unexpectedly** (e.g. a leaf needs another lane's not-yet-landed work): that's a planning bug; pause that lane and re-sequence.

## Estimated wall-clock

| Wave | Parallel | Sequential | Wall-clock (upper) |
|---|---|---|---|
| 0 | 4 designs | — | ~half-day |
| 1 | 4 IMPLs (1L, 2S, 1M) | — | ~1.5 days (gated by B) |
| 2 | 3 IMPLs (1S, 1L, 1M) + B sequences | B serial after A | ~3 days |
| 3 | 3 IMPLs (1L, 1L, 1M-L) + B BENCH | — | ~3-4 days |
| 4 | 4 (smokes + idle) | A→B→C smoke.sh sequence | ~1 day |
| 5 | DOCS consolidation | sequential | ~half-day |
| **Total** | | | **~9-10 days wall-clock** |

Sequential equivalent (sum of upper bounds across all 32 leaves): **~22-27 person-days**.

**Speedup: ~2.5-3×**, gated by Lane C (snapshot/restore) length and the contested `launch.rs` serializations.

## What I will NOT do without checking with you

- Dispatch the wire-protocol epics (`m80-5vha`, `m80-6zim`) under this plan. They're parallel work but explicitly out of scope.
- Skip a smoke checkpoint to save time. The whole point of the perf-roadmap-extended doc is that smokes gate BENCH.
- Land m80-rrp.3 (snapshot) leaves before m80-f2zc (storage pivot) lands. The cross-epic dep is intentional.
- Promote any persistent-VM leaf to land before the cancellation contract is settled (qokt.2.4 is the pinch point).

## Recommendation

Begin with **Wave 0**: 4 parallel sonnets, all writing fresh design/research docs to non-overlapping paths. It's the lowest-risk wave (no code), the highest-leverage wave (locks contracts every later wave consumes), and confirms the worktree-isolation mechanics before anything contested moves.

If Wave 0 lands cleanly within ~half-day, Wave 1 can dispatch the same evening. If anything in Wave 0 throws unexpected surface area, we adjust before code is in flight.
