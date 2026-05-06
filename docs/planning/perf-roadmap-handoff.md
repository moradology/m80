# Prompt: m80 perf-roadmap — current state and remaining work

You're picking up the m80 perf-roadmap mid-stream. m80 is a Rust-embeddable Firecracker microVM sandbox at `/tank/projects/m80`, targeting agent infrastructure. Goal: sub-200 ms warm-pool launch latency.

## What you need to read first (in order)

1. **`/tank/projects/m80/CLAUDE.md`** — project conventions. Especially "What we don't want" (no defensive ceremony, no premature abstraction, no silent recovery) and "When debugging — diagnostics before hypotheses".
2. **`/tank/projects/m80/docs/planning/agent-roadmap.md`** — the four-step path; the "Post-roadmap state (2026-05-05)" section at the bottom is your current-state summary.
3. **`/tank/projects/m80/CHANGELOG.md`** `[Unreleased]` — consolidated entry listing every IMPL that landed across Waves 0-4 with file paths + brief mechanism per epic.
4. **`/tank/projects/m80/docs/planning/perf-roadmap-extended.md`** — risk registers and ID map. Reach for this when a bead's intent is unclear.
5. **`/tank/projects/m80/docs/planning/parallel-execution-strategy.md`** — lane/file-territory map (only relevant if you're dispatching more parallel work).

## Where we are

Five waves of parallel sonnet dispatches landed structurally over commits `33d725e..fa52206` (25 commits). All four perf-roadmap epics are **code-complete**:

| Epic | Bead | What landed |
|---|---|---|
| Storage pivot | `m80-f2zc` | Sparse overlay + in-guest overlayfs + `pivot_root` (kata lift) |
| Stripped kernel | `m80-ci9i` | Kernel-builder Dockerfile, schema v3, two-axis boot_args |
| Snapshot/restore | `m80-rrp.3` | REST surface, capture/restore primitives, `--from-snapshot` CLI |
| Persistent VM | `m80-qokt.2` | `&mut self` exec, Cancel envelope, idle-timeout watcher |

**Workspace:** `cargo test --workspace` 93 pass / 0 fail / 4 ignored (KVM-gated); `cargo clippy --workspace --all-targets -- -D warnings` clean.

**What happened after the original handoff:** real-KVM exercise is now done for
the storage pivot, snapshot restore, persistent-VM, and stripped-kernel paths.
The stripped-kernel Ubuntu failure was fixed by adding the systemd API
filesystem keep-list to the kernel config.

## Current bench state

The original handoff listed four **BENCH** leaves. Three now have real-KVM
measurements and are closed:

- `m80-f2zc.7` — storage pivot: minimal idle N=30 P50 1517 ms, storage prep
  215.9 ms; loaded cell 1/5 success; 16-VM concurrent probe passed 16/16.
- `m80-rrp.3.6` — snapshot restore: idle N=50 P50 274.204 ms / P95
  280.132 ms; loaded N=50 P50 444.972 ms / P95 588.221 ms.
- `m80-qokt.2.6` — persistent VM: sequential exec N=30 P50 20.589 ms /
  P95 20.716 ms.

- `m80-ci9i.4` — stripped kernel: minimal idle N=30 P50 1417 ms, Ubuntu
  idle N=30 P50 3818 ms, minimal loaded 0/5 success under full CPU
  saturation.

| Bead | Captures | Prereq |
|---|---|---|
| none | All originally listed BENCH leaves are measured | Completion audit still needs to verify tracker/docs/tests before closing the active goal |

## How to do the bench work

The pattern in roughly the order that minimizes throwaway work:

1. **Cross-compile m80-guestd** to musl (existing image-build pipeline knows how — see `crates/m80-image-build/src/minimal.rs`). The new guestd has the overlay+pivot logic from `m80-f2zc.4`.
2. **Rebuild the minimal image** with the new guestd: use `M80_IMAGE_KIND=minimal ./scripts/smoke.sh` or run `m80-image-build run --config <toml>` directly.
3. **Run the existing smoke** to validate the new overlay+pivot end-to-end: `./scripts/smoke.sh` with `M80_IMAGE_KIND=minimal`. **If this fails, do diagnostics-first triage per CLAUDE.md** — make guestd's stderr visible (already wired via `eprintln!` in `pid_one.rs`) and find the failing step before forming hypotheses.
4. **Bench the storage pivot impact** (`m80-f2zc.7`) — done; see `docs/perf/cold-launch.md`.
5. **Build and bench the stripped kernel** (`m80-ci9i.4`) — done; see `docs/perf/cold-launch.md`.
6. **Snapshot round-trip bench** (`m80-rrp.3.6`) — done; see `docs/behaviors/snapshot/restore-latency.md`.
7. **Persistent-VM bench** (`m80-qokt.2.6`) — done; see `docs/behaviors/lifecycle/persistent-state.md`.

Each bench leaf wants P50 numbers in `docs/perf/cold-launch.md` and a brief CHANGELOG entry. Per CLAUDE.md: do NOT fabricate numbers; if a step fails or is non-trivial, report what you got and why.

## Things to know that aren't obvious from the docs

- **Schema v3 backward compat**: `Manifest.kernel_kind` has `#[serde(default)]` — existing v2 manifests deserialize fine. But Rust struct literals still need the field; `crates/m80-preflight/tests/render_table.rs` had this gap, fixed in `fa52206`. If you find another, fix it the same way.
- **`8250.nr_uarts=1` is locked** in the Stripped cmdline (not `=0`). This costs ~50 ms but preserves console diagnostics. Don't optimize it away; CLAUDE.md "diagnostics before hypotheses" outweighs it.
- **Cancel envelope is shared** between `m80-qokt.2.4` (landed) and `m80-5vha` (streaming exec, separate epic, NOT landed). The proto types in `m80-proto::types::{CancelRequest, CancelAck, CancelStatus}` are stable; m80-5vha will reuse them unchanged.
- **vsock survival across snapshot**: connections die (TRANSPORT_RESET); LISTEN sockets survive. The restore path uses `phase_restore_probe_exec_channel` (in `crates/m80-firecracker/src/launch.rs`), NOT the cold-boot inverted-readiness pattern. Don't get confused if you read the m80-7tpy code and wonder why restore doesn't use it. See `docs/exploration/firecracker-vsock-snapshot.md` for the empirical evidence.
- **Idle timeout signals via flag, not direct shutdown**: the watcher sets `Arc<AtomicBool>` `idle_timed_out`; the next `exec` call observes it and returns `FcError::IdleTimedOut`; caller drops to trigger Drop-shutdown. Watcher does NOT call `stop()` directly (no shared mutable access). See `crates/m80-firecracker/src/lifecycle.rs`.
- **`m80 snapshot capture` CLI is a v0.1 stub** (exits 7 with an explanation) — the live capture path needs host IPC plumbing that isn't there yet. The library API (`RunningSandbox::capture`) works; only the standalone CLI flow is stubbed. For benching, drive capture from a Rust test, not the CLI.
- **No worktree isolation in this env**: previous parallel dispatches used `CARGO_TARGET_DIR=/tmp/m80-build/target-w<N>-<lane>` to avoid cargo-build-lock contention. If you dispatch more sonnets in parallel, do the same.

## What the user wants

- **Don't escalate trivia.** The user's standing instruction this session: "only bring to me HIGH PRIORITY DECISION MAKING." Obvious calls (commit messages, fixture fixes, bead state, sequential vs parallel when path is clear) — just do them.
- **Diagnostics first when something breaks.** A failed launch is not a guessing game. Make the failing layer's stderr visible before forming hypotheses.
- **Smallest surface that satisfies the bead.** No "while I'm here" cleanups. No defensive shims. No premature abstraction. Three repeated lines beat a generic helper.

## How to drive a bead

```
br show <bead>                            # read acceptance criteria
br update <bead> --claim                  # claim it
# … do the work …
br update <bead> --notes "commit=<sha>; …"
br close <bead> --reason "implemented"    # or "captured" for behavior leaves
git add .beads/issues.jsonl               # stage bead state alongside code
```

Workflow tooling: `br --help`, `bv --robot-triage` for what's open. Bead authoring tools live under `specs/` if you need to file new leaves.

Good luck. The code is in place; what remains is honest empirical exercise.
