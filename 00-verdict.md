# 00 — Verdict

## Recommendation

**Yes, extract it.** The shape is clear, the bones are clean, and the cost is
bounded. Ship it in two phases over 4–6 weeks. The benefit is bilateral: a
useful focused public artifact, and a cleaner predecessor (the FC backend stops
being a leaky abstraction inside the agent platform).

## Why this works

1. **The mechanics are mostly portable.** ~70% of the firecracker crate is
   generic Linux+Firecracker mechanics: process spawn, UDS API client, vsock
   transport, ext4 cloning, NAT/iptables setup, jailer chroot, cgroup v2.
   None of it knows or cares that the caller is an agent platform.

2. **The predecessor coupling is shallow.** It's metadata threading
   (`workspace_id`, `run_id`), an `EffectClass` enum, a tiny `CapabilityClass
   → VmNetworkMode` resolver, and a writeback-authority hook that's *already*
   parameterized behind a trait. None of it drives lifecycle branching.

3. **A real abstraction already exists.** `agent-sandbox-api::SandboxBackend`
   has three implementations (local, container, firecracker). The trait is
   lean (`async fn execute(req) -> Result<resp, err>`). It's been
   pressure-tested across subprocess models. The catch: its `ExecutionRequest`
   carries predecessor metadata that a generic CLI shouldn't impose on users.

4. **Dead/reserved code is identifiable and droppable.** `snapshot.rs`
   (997 LOC) is drafted but never invoked from `lifecycle.rs` —
   `UnsupportedSnapshotLaunchMode` proves it. `blank_pool.rs` (1,412 LOC) is
   exported but `backend.rs` never instantiates the allocator. That's
   ~2,400 LOC of code that can be deleted on day one.

5. **Only two direct consumers inside predecessor.** `worker-rs` and
   `sandbox-executor-rs` import `agent_sandbox_firecracker` types directly.
   Everything else (`agent-sandbox-executor-core`, `agent-tool-executor`,
   `agent-sandbox-executor-nats`) talks to the trait. Re-stubbing is mechanical.

## Cost estimate

Single engineer, focused, with prior context on the codebase:

| Slice | Estimate | Notes |
|---|---|---|
| Extract host-side core; replace predecessor IDs with opaque strings; drop authority hook default | **1–2 weeks** | The predecessor metadata threads through everywhere but mostly just gets logged or persisted |
| Redesign guest daemon + protocol around generic exec | **3–5 days** | Guest is ~2,600 LOC; new generic surface is ~500 LOC |
| Generalize image build pipeline (rootfs, manifest, preflight) | **3–5 days** | `prepare-guestd-image.sh` + `firecracker-preflight.sh` are well-structured |
| CLI surface (`m80 run`, `m80 shell`, `m80 prepare-image`, `m80 preflight`) | **3–5 days** | Underlying primitives all exist |
| Tests, CI on Linux/KVM runner, docs | **1 week** | KVM-required CI is the friction point |
| Stitch predecessor back via thin `m80-adapter` crate | **2–4 days** | Adds back workspace_id, authority, tool catalog as a thin layer |

**Net: 4–6 weeks of focused work** for a publishable v0.1 with CLI.

If you're willing to ship without the production-hygiene tail (diagnostics,
scrape, probe, readiness, snapshot, blank-pool), drop ~1 week.

## Real risks (in order)

1. **Networking module size and shell-out approach.** `network.rs` is
   3,669 lines, shells to `ip`/`iptables`, expects effective root or
   passwordless `sudo`, and bakes in predecessor's run-root directory layout for
   collision detection. It works because predecessor's conventions are stable;
   a new home will fight some assumptions. Plan for **1–2 weeks just for
   this file** if `OutboundNat` is in scope. Defer it for v0.1 if possible.

2. **KVM + privilege requirements limit "low-effort".** The CLI Just Works
   on a Linux host with `/dev/kvm` and root/sudo; on macOS it requires Lima
   with nested virt. There is no way around this — write it into the README
   prominently and don't pretend otherwise.

3. **Image build pipeline must come along.** Today it pulls Ubuntu rootfs
   from S3, kernel from a firecracker-ci S3 bucket, and the firecracker
   tarball from GitHub releases. A useful CLI ships a single
   `m80 prepare-image` that produces a deterministic, hashable rootfs;
   otherwise users can't actually boot anything they didn't already have.

4. **Test footprint.** 128 references to `LEGACY_FC_INTEGRATION` /
   `LEGACY_K8S_FIRECRACKER_*` across the codebase. Most are in
   `agent-sandbox-firecracker/tests/` and stay with the crate, but a handful
   live in `worker-rs` and `sandbox-executor-rs` and need to be re-stubbed
   against the adapter or removed.

5. **Inside predecessor: the `placement_state.observe_materialized` callback.**
   `worker-rs/src/executor_factory.rs` wraps the firecracker backend in a
   tracking adapter that pings placement state. The new lib won't know about
   placement, so the tracking wrapper has to live in
   `m80-adapter`. Easy, but don't forget it.

## Phasing

**Phase 1 — m80 v0.1** (~2–3 weeks):
- Crystalline core only: foundation, boot, lifecycle, client, vsock,
  storage, network (NoEgress only), jailer, cgroup, errors (trimmed)
- Generic-exec guest daemon
- CLI: `run`, `shell`, `exec`, `ls`, `stop`, `logs`, `prepare-image`,
  `preflight`
- Linux/KVM only, single-VM at a time, `NoEgress` only
- Drop `snapshot.rs`, `blank_pool.rs`
- Hold off on `diagnostics`, `scrape`, `probe`, `health`, `readiness`,
  `ops_metrics`

**Phase 2 — m80 v0.2** (~2–3 weeks more):
- Optional observability under feature flags
- `OutboundNat` networking
- Snapshot/restore (real this time, not the unfinished shell)
- Warm-pool / blank-VM allocator
- Multi-VM orchestration in the CLI

**Phase 3 — predecessor cutover** (~2–4 days):
- Replace `agent-sandbox-firecracker` with `m80-core` + thin
  `m80-adapter`
- Keep the existing `SandboxBackend` trait edge intact
- Re-stub the two direct consumers and the test suite

## What's *not* in scope

- Replacing predecessor's `SandboxBackend` trait. The trait stays where it is;
  m80 ships its own simpler trait, and the adapter bridges them.
- Replacing predecessor's `agent-guest-proto`. The adapter can keep speaking the
  predecessor wire format on top of m80's transport.
- Replacing the `agent-sandbox-tool-catalog` and `agent-tool-executor`
  layers. Those stay in predecessor — they're agent-platform concerns, not
  sandbox concerns.
