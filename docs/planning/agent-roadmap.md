# Agent-use-case roadmap

**Goal:** sub-200 ms warm-pool launch latency for the agent use case.
**Current baseline (2026-05-04, post-7tpy):** minimal/idle 1.9 s, ubuntu/idle 6.4 s.
**Reliability:** 100 % at N=20 ubuntu/idle (was 50-60 % pre-7tpy).

After hard look at competition (smolvm/libkrun, kata-containers, e2b
SaaS), m80's defensible niche is **self-hosted, Rust-embeddable,
Firecracker-based, agent-shaped sub-second warm pool**. To occupy it,
we follow the bead chain below — each step has a known mechanism and
documented expected savings.

---

## Latency budget today

Per-phase P50 (minimal/idle, N=15, post-7tpy):

```
phase_3_storage_prep              770 ms   ← rootfs full file copy (256 MiB)
phase_12b_ready_accept            890 ms   ← kernel boot + guestd start
phase_9_jailer_launch              25 ms
phase_5b_cgroup_create             17 ms
phase_12a_instance_start           18 ms
exec_recv                          11 ms
stop_bounded                       54 ms   ← vsock graceful + SIGKILL
                                ──────
sum                              ~1700 ms (rest is bench/CLI wrapper overhead)
total wallclock P50              ~1900 ms
```

`phase_3_storage_prep` and `phase_12b_ready_accept` together are
**88 % of useful_ms**. Both have planned fixes.

---

## The path to sub-200 ms (in order)

| # | Step | Bead | Expected save | After this |
|--:|---|---|---:|---:|
| 1 | **Storage pivot** (RO base + per-VM overlay + in-guest overlayfs) | `m80-f2zc` | ~760 ms | **~1.1 s** |
| 2 | **Stripped kernel** (minimal config + cmdline trim) | `m80-ci9i` (new) | ~500-700 ms | **~400-600 ms** |
| 3 | **Snapshot/restore** (Firecracker memory snapshot + warm pool) | `m80-rrp.3` | replaces cold boot entirely; ~125-200 ms restore | **cold ~400 ms; warm ~200 ms** |
| 4 | **Persistent VM** (multi-exec on one VM) | `m80-qokt.2` | ~0 between turns within a session | **~0 turn-to-turn** |

Steps 1-2 are independent and can land in either order. 3 depends on
storage being mature (overlay clean for snapshot). 4 is orthogonal and
can land anytime.

For ubuntu/idle (currently 6.4 s, dominated by systemd boot): steps
1-2 still help (storage save applies; kernel save is more modest
because systemd init is the bottleneck). Honest assessment: **ubuntu
is the wrong path for agent-grade latency**; minimal-with-snapshot is
where the agent UX lives. Ubuntu stays as a familiar-userspace
fallback.

---

## Wire-protocol features (parallel track, not on the latency path)

These are needed for **agent UX** (not latency) — agents do file ops
and long-running execs constantly, today's wire forces base64+bash for
each:

| Feature | Bead | Why |
|---|---|---|
| File-ops verbs (read/write/list/stat/chunked-write) | `m80-6zim` | Smolvm has these; agents move files constantly |
| Real streaming exec (multi-frame stdout/stderr) | `m80-5vha` | Smolvm's is fake (buffered); we'd be the only real one |

These don't affect launch latency but are agent-UX must-haves. P2,
file in parallel.

---

## Bead ID ↔ logical name map

The bead system auto-generates IDs; planning docs reference logical
names. Cross-reference:

| Logical | Actual | Status (2026-05-04) |
|---|---|---|
| Storage pivot | `m80-f2zc` (epic + 8 leaves) | open, P2 |
| Stripped kernel | `m80-ci9i` (epic + 5 leaves) | open, P2 |
| Snapshot/restore | `m80-rrp.3` (sub-leaf of `m80-rrp` warm-pool epic) | open, P1 |
| File-ops verbs | `m80-6zim` (epic + 6 leaves) | open, P2 |
| Streaming exec | `m80-5vha` (epic + 6 leaves) | open, P2 |
| Persistent VM | `m80-qokt.2` (epic, sub-leaves not yet filed) | open, P1 |
| Inverted readiness | `m80-7tpy` (closed, ad00c02) | done |
| Quick-stop fix | `m80-bgas` family + the `panic=-1` quick-win commit | done |

`docs/planning/storage-pivot-bead-plan.md` and
`docs/planning/wire-features-bead-plan.md` are the authoritative
narrative for the storage pivot and the two wire epics respectively.

**`docs/planning/perf-roadmap-extended.md`** (filed 2026-05-05) extends
each of the four latency epics with explicit risk registers, post-IMPL
smoke checkpoints, rollback notes, cross-epic dependency edges, effort
ranges with upper bounds, and confidence-intervaled savings. That doc
is the source of truth for the extended bead state; it includes an
aspirational-ID → actual-ID map at the end.

---

## Recent reliability win (for context)

The 40-50 % ubuntu/idle launch failure rate that haunted the bench for
most of 2026-05-04 had **two compounding causes**, both fixed in
commit `ad00c02`:

1. **Polled-CONNECT/OK probe race**: the host's 10 ms-cadence vsock
   probe provoked an EAGAIN race in Firecracker's vsock muxer accept
   loop. Replaced by inverted readiness (`m80-7tpy`).
2. **Service-unit ordering cycle**: `m80-guestd.service` was
   `WantedBy=multi-user.target` + `After=multi-user.target`, dragging
   in `network-wait-online` whose timeout is bimodal (~1.5 s vs full
   90 s). On the bad path m80-guestd started at T=91 s — past the
   host's READY_TIMEOUT.

Both fixed. Now N=20 = 20/20 pass. The 7tpy work also enabled
tearing out all polled readiness code (m80-vsock lost
`watch_ready_marker`, the 5-arg `Channel::open`, `READY_POLL_INTERVAL`,
the console-marker test fixture). Cleaner code surface.

CLAUDE.md gained a "When debugging — diagnostics before hypotheses"
section so the next bug doesn't repeat the hour I spent forming
theories before adding `StandardError=journal+console` to the service
unit.

---

## How to follow this roadmap

1. **Pick the next item from the priority table above.** Items 1-2
   are P2 cold-path; item 3 is P1 (the headline agent-UX unlock); item
   4 is P1 (multi-turn within a session).
2. **Read the bead's parent description** (e.g. `br show m80-f2zc`)
   for the full context.
3. **Read the linked planning doc** for the multi-leaf design — these
   docs are the source of truth for sequencing within an epic.
4. **For each leaf**: claim, do, close per the m80 bead workflow. The
   in-flight `useful_ms` should drop after each step; capture the
   numbers in `docs/perf/cold-launch.md` and the bead's BENCH leaf.

Re-run `./scripts/bench-cold-launch.sh` after each epic lands to keep
the budget honest. The harness writes per-launch CSV + per-phase CSV
that can be diffed against the pre-epic baseline.

---

## Post-roadmap state (2026-05-05)

Waves 0-4 of the parallel implementation strategy landed at the
**structural** level, and the follow-up real-KVM bench pass has now measured
the four originally open BENCH leaves. The stripped kernel needed one
diagnostics-first correction for Ubuntu/systemd: add cgroups, tmpfs ACL/xattr
support, file-handle syscalls, and systemd event primitives to the keep-list so
systemd can mount its API filesystems.

| Bead | What it captures |
|---|---|
| `m80-f2zc.7` | Storage-pivot cold-launch numbers: closed; storage prep P50 727.6 ms → 215.9 ms |
| `m80-ci9i.4` | Stripped-kernel cold-launch numbers: closed; minimal idle P50 1517 ms → 1417 ms, Ubuntu idle P50 3919 ms → 3818 ms |
| `m80-rrp.3.6` | Snapshot/restore wallclock: closed; idle restore P50 274.204 ms, loaded restore P50 444.972 ms |
| `m80-qokt.2.6` | Persistent-VM turn-to-turn: closed; sequential exec P50 20.589 ms |

The empirical numbers are in `docs/perf/cold-launch.md`,
`docs/behaviors/snapshot/restore-latency.md`, and
`docs/behaviors/lifecycle/persistent-state.md`.

DOCS leaves (`m80-f2zc.8`, `m80-ci9i.5`, `m80-rrp.3.7`) closed during
Wave 5 — READMEs were updated alongside each IMPL diff per the CLAUDE.md
rule, and `CHANGELOG.md` has the consolidated [Unreleased] entry.

See `docs/planning/perf-roadmap-extended.md` for risk registers and the
ID map; `docs/planning/parallel-execution-strategy.md` for the lane/file-
territory map that drove the parallel dispatch.
