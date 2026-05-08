# Bestiary + conveyor-belt VM pool — future direction

> **Status:** Future direction. **Outside m80's core scope per CLAUDE.md** (m80
> captures VM mechanics only). This document specifies a multi-tenant
> orchestration layer that sits *above* m80 and consumes m80's primitives. The
> m80-side primitives this design depends on are tracked by **m80-iswt**.
>
> Source: `/tmp/m80-conveyor-belt-strategy.md` (hermes, 2026-05-07).

## Overview

The conveyor belt is m80's strategy for serving multi-tenant sandboxed workloads
with hardware-level isolation, sub-50 ms allocation latency, and a cryptographic
guarantee that no tenant's data can ever leak to another tenant.

The core idea: the pool is a **conveyor belt, not a parking lot**. VMs enter
blank, get used once with a single tenant's data, and are destroyed on the way
out. The pool is continuously refilled in the background with fresh generic VMs
that have never touched any tenant's data.

The orchestration layer that drives this is called the **Bestiary** — a
separate Rust program/daemon that calls m80's API. m80 stays a pure execution
engine; the Bestiary owns pools, tenants, profiles, and strategy enforcement.

---

## Why this works: security

### The threat

In a multi-tenant sandbox system, the worst possible failure is cross-tenant
data leakage. If tenant A's data is somehow visible to tenant B — through stale
filesystem cache, leftover in-memory buffers, orphaned mount state, or open
file descriptors — the isolation guarantee is broken.

This threat exists in any system that reuses execution environments between
tenants. The question is not whether cleanup *can* work, but whether you can
*prove* it always works, even under crashes, timeouts, and edge cases.

### Why the conveyor belt eliminates this class of bugs entirely

**A VM that is destroyed cannot leak data.**

Tautological but powerful. Instead of trying to perfectly clean a VM between
tenants — unmounting, flushing caches, zeroing memory, closing file
descriptors, verifying mount-namespace cleanliness — we never attempt reuse.
The VM is killed. Its process is gone. Its memory is freed. Its mount namespace
ceases to exist. There is no state to clean because there is no process.

This converts a *correctness* problem (did we clean everything?) into an
*operational* problem (can we refill fast enough?). Correctness problems
require exhaustive testing and can still have edge cases. Operational problems
are solved by having enough capacity, which is straightforward to verify.

### The security chain, step by step

1. **Pool VMs are generic.** They contain only the base rootfs, guestd, and a
   kernel. No tenant data has ever been inside them. They are restored from a
   known-good snapshot image stored on disk.
2. **Tenant data enters only at allocation time.** When a request arrives for
   tenant X, the pool manager grabs a generic VM and attaches tenant X's block
   device via Firecracker's `PATCH /drive` API.
3. **Tenant identity is verified.** After mounting the block device, guestd
   reads a caller-specified path on the tenant disk and sends the bytes to
   the host over the vsock channel (m80 carries opaque bytes; the path is
   configurable per-attach via the `identity_path` field on the hot-plug
   request). The host compares the reported bytes against the expected tenant.
   Mismatch = immediate kill.

   **What m80 owns vs. what the Bestiary owns.** m80 provides the transport
   and the byte-compare; **the threat model and any cryptographic structure of
   the identity bytes are the Bestiary's responsibility**, not m80's. Choices
   the Bestiary must make:
   - **Threat model.** Is verification defending against (a) Bestiary's own
     bookkeeping bugs, (b) disk-swap during attach, or (c) guest VM tampering
     with the workspace? (a) and (b) need only opaque-bytes comparison; (c)
     requires crypto-signed tokens with a key the workload doesn't have.
   - **Token shape.** UUID, signed JWT, sealed nonce, filesystem-UUID
     reflection, or virtio-blk serial. m80 doesn't care; it carries bytes.
   - **Where the identity lives.** A file at `/workspace/.tenant-identity`
     is the convention used in the m80 tests, but the path is just bytes
     coming back over the wire — `/tenant/.tenant-identity`,
     `/.identity`, or any other path works equally.
   - **Pre-attach vs. post-attach population.** The Bestiary can write the
     identity to the disk file pre-attach (host-side, before PATCH /drive)
     or rely on the disk image already containing it. Either way, m80 just
     reads from `identity_path` after the guest mounts the disk.
4. **The VM serves exactly one workload for exactly one tenant.** No second
   mount, no second tenant, no cleanup step.
5. **The VM is destroyed after the workload completes.** Kill the Firecracker
   process. Address space, fd table, mount namespace, and kernel state are
   gone. The only surviving artifact is the tenant's own block device, which
   persists independently.
6. **A fresh generic VM replaces it.** Snapshot restore creates a brand-new VM
   from the known-good base image. It has never seen any tenant's data.

### What this guarantees

| Guarantee | Mechanism |
|---|---|
| No cross-tenant filesystem leakage | VM is destroyed, not reused |
| No cross-tenant memory leakage | Process killed, address space freed |
| No stale mount state | Mount namespace ceases to exist |
| No open file descriptor leakage | File descriptor table destroyed with process |
| No kernel cache leakage | VM kernel state destroyed with the VM |
| Correct tenant always served | Cryptographic identity verification after mount |

### What happens on failure

If anything goes wrong during allocation — mount fails, identity doesn't match,
guestd doesn't respond — the VM is killed and discarded. Grab another from the
pool. The failure mode is a latency spike for one request, never a security
incident.

---

## Why this works: latency

### The three latency paths

| Path | P50 latency | When used |
|---|---|---|
| Cold boot (from scratch) | ~1117 ms | Never on request path — only for initial snapshot capture |
| Snapshot restore + drive attach | ~300-400 ms | Background pool refill |
| Warm pool allocate + drive attach | ~45 ms | Request path |

### How 45 ms is achieved

A warm pool VM is already booted with guestd ready and listening on vsock. The
only work at allocation time is:

1. **Dequeue from pool** — grab a VM handle. ~0 ms.
2. **PATCH /drive** — Firecracker attaches the tenant's block device via
   virtio-blk hotplug. ~5 ms.
3. **Guest mount** — guestd detects the new block device via udev/uevent,
   mounts it, reads the tenant identity file, sends verification over vsock.
   ~15-25 ms.
4. **Host verifies identity** — compare reported identity against expected.
   ~0 ms.
5. **Signal ready** — execute the workload.

Total: ~40-50 ms from request to execution start.

### Why pool refill doesn't affect the request path

Pool refill happens in a background task. When a VM is consumed from the pool,
the refill process immediately starts a snapshot restore to replace it. ~274 ms
for the restore itself, plus ~25-100 ms for drive attach and mount verification
of a test mount (if desired) — totaling ~300-400 ms.

But this 300-400 ms happens *in parallel* with workload execution. If the
average workload takes 2 seconds, refill has 2 full seconds to complete.

### Elastic buffer keeps the pool full under load

The pool maintains a buffer of N idle VMs above current active concurrency
(N = 1 or 2, configurable). Under steady load:

- A request consumes one VM from the buffer (45 ms allocation)
- Background refill restores one VM to the buffer (300-400 ms, but overlapping
  with the workload's execution time)
- Since workloads take longer than refill, refill stays ahead

Under burst load (e.g., 10 simultaneous requests when the pool has 2 idle VMs):

- First 2 requests get the fast 45 ms path
- Remaining 8 pay the snapshot restore cost (~300-400 ms)
- Refill kicks in immediately; subsequent requests get the fast path again as
  refill catches up

### Scale-to-zero means no wasted resources when idle

After a configurable idle timeout (e.g., 30-60 seconds with no requests), the
pool stops refilling. The last VMs get consumed by their workloads and
destroyed; no new ones are created. Zero VMs running, zero RAM consumed.

When the next request arrives at an empty pool, it pays ~300-400 ms for a
snapshot restore. Acceptable for CLI / dev use; the pool immediately starts
building the buffer back up so subsequent requests are fast.

---

## Architecture diagram

```
                ┌─────────────────────────────────┐
                │         VM PROFILE SNAPSHOTS     │
                │   (on disk, zero RAM when idle)  │
                │                                  │
                │  minimal-128.snap  (stripped)    │
                │  minimal-256.snap  (stripped)    │
                │  full-1024.snap    (stock)       │
                └──────────┬──────────────────────┘
                           │
                ┌──────────▼──────────────────────┐
                │       BACKGROUND REFILL          │
                │                                  │
                │  snapshot_restore(profile)       │
                │  → booted VM, guestd ready       │
                │  → NO tenant disk attached       │
                │  → enters generic pool           │
                │                                  │
                │  latency: ~274 ms per VM         │
                └──────────┬──────────────────────┘
                           │
                ┌──────────▼──────────────────────┐
                │         GENERIC POOL             │
                │                                  │
                │  [VM-A] [VM-B] [VM-C] ...       │
                │                                  │
                │  All blank. All identical.       │
                │  No tenant state.                │
                │  Elastic depth: active + buffer  │
                └──────────┬──────────────────────┘
                           │
      ┌────────────────────┼────────────────────┐
      │                    │                    │
┌─────▼─────┐        ┌─────▼─────┐        ┌─────▼─────┐
│  REQUEST  │        │  REQUEST  │        │  REQUEST  │
│  tenant X │        │  tenant Y │        │  tenant Z │
│           │        │           │        │           │
│ 1. alloc  │        │ 1. alloc  │        │ 1. alloc  │
│ 2. PATCH  │        │ 2. PATCH  │        │ 2. PATCH  │
│   /drive X│        │   /drive Y│        │   /drive Z│
│ 3. mount  │        │ 3. mount  │        │ 3. mount  │
│ 4. verify │        │ 4. verify │        │ 4. verify │
│ 5. exec   │        │ 5. exec   │        │ 5. exec   │
│ 6. DESTROY│        │ 6. DESTROY│        │ 6. DESTROY│
└───────────┘        └───────────┘        └───────────┘

Each VM serves EXACTLY ONE tenant, then dies.
No reuse. No cleanup. No leakage possible.
```

---

## Tenant disk attachment: the 45 ms allocation path in detail

```
t=0ms     Request arrives for tenant X
          Pool manager dequeues a generic VM

t=0ms     PATCH /drive via Firecracker API
          → virtio-blk device hotplug into running VM
          ~5 ms

t=5ms     Guestd detects new block device (udev/uevent)
          → reads device path
          ~2 ms

t=7ms     Guestd mounts block device at /workspace
          → mount(idempotent, 100 ms timeout)
          ~10-15 ms

t=22ms    Guestd reads the caller-configured `identity_path` on the
          mounted tenant disk (e.g., /workspace/.tenant-identity)
          → opaque bytes (UUID, signed token, or whatever the
            Bestiary chose — m80 doesn't interpret)
          → sends over vsock to host
          ~3 ms

t=25ms    Host verifies identity
          → matches expected tenant X?
          → YES: signal ready
          → NO:  kill VM, try next pool slot
          ~1 ms

t=26ms    Execute tenant X's workload
          ~arbitrary (seconds to minutes)

t=done    DESTROY VM
          → kill Firecracker process
          → entire VM state ceases to exist
          → trigger background refill

t=done+1  Background: snapshot_restore begins
          → ~300-400 ms to replace consumed VM
          → overlaps with other workloads, not on request path
```

---

## Comparison to alternative strategies

### Sticky per-tenant pools (Model A)

Each tenant gets dedicated VM slots pre-mounted with their disk.

- **Latency:** ~21 ms (just allocate, disk already mounted)
- **Security:** Good — tenant never shares a VM. But VM is reused between the
  tenant's own workloads, requiring cleanup between runs.
- **RAM cost:** Scales with total tenant count, not concurrency. 200 tenants ×
  2 slots × 256 MiB = ~100 GiB of RAM for idle capacity.
- **Flexibility:** Changing VM profiles requires draining and rebuilding
  per-tenant slots.

### Generic pool with on-demand mount (conveyor belt — Model B)

Shared pool of identical VMs. Tenant disk attached at allocation time.

- **Latency:** ~45 ms (allocate + drive attach + mount + verify)
- **Security:** Strictest — VM is destroyed after every workload. No cleanup
  needed. Cryptographic identity verification. No state can persist.
- **RAM cost:** Scales with actual concurrency, not tenant count. 10
  concurrent workloads × 256 MiB = ~2.5 GiB regardless of total tenant count.
- **Flexibility:** Different VM profiles coexist as separate snapshot files.
  Pool manager maintains buffers per active profile.

### Why conveyor belt wins

| Dimension | Sticky pools | Conveyor belt |
|---|---|---|
| Cross-tenant leakage | Requires correct cleanup | Impossible (VM destroyed) |
| RAM at 200 tenants, 10 concurrent | ~100 GiB | ~2.5 GiB |
| P50 allocation latency | ~21 ms | ~45 ms |
| Worst case (empty pool) | ~274 ms (restore) | ~300-400 ms (restore + mount) |
| Profile flexibility | Per-tenant reconfiguration | Instant — different snapshot |
| Correctness burden | High — must prove cleanup | Low — no cleanup to prove |

The 24 ms latency difference (45 ms vs 21 ms) is negligible for code-execution
workloads. The RAM savings and security guarantee are decisive.

---

## The Bestiary: orchestration layer

The conveyor-belt strategy is implemented in a separate Rust program called
the **Bestiary**. m80 provides the primitives — launch, snapshot, restore,
exec, destroy. The Bestiary provides the orchestration — pool management,
tenant routing, profile selection, and strategy enforcement.

m80 knows nothing about tenants, pools, or strategies. It is a pure execution
engine. The Bestiary calls m80's API to perform each step.

### Bestiary responsibilities

1. **Pool lifecycle** — maintain the elastic buffer, trigger background
   refills, enforce idle timeout for scale-to-zero.
2. **Tenant routing** — receive requests, identify the tenant, select the
   correct VM profile, allocate a pool VM, attach the tenant's disk.
3. **Identity verification** — compare guestd's reported tenant identity
   against the expected tenant. Kill on mismatch.
4. **Profile management** — track which VM profiles have recent demand;
   maintain per-profile buffer sizes.
5. **Strategy enforcement** — ensure VMs are destroyed after use (never
   reused), enforce mount timeout, handle allocation failures by discarding
   VMs.

### Bestiary config (example)

```toml
[pool]
buffer = 2                    # idle VMs above active concurrency
idle_timeout_secs = 60        # scale to zero after this
max_pool_size = 50            # hard cap on total VMs
mount_timeout_ms = 100        # kill VM if mount takes longer

[profiles.minimal_128]
snapshot = "/var/lib/m80/snapshots/minimal-128mb.snap"
ram_mib = 128
cpu_template = "C3"

[profiles.minimal_256]
snapshot = "/var/lib/m80/snapshots/minimal-256mb.snap"
ram_mib = 256

[strategy]
type = "conveyor_belt"        # default and recommended
verify_tenant = true          # cryptographic identity verification
one_shot = true               # destroy VM after every workload
```

### CLI integration sketch

```bash
# Without bestiary daemon — direct snapshot restore (~300-400 ms)
$ m80 exec --profile minimal-128 --tenant-x -- ./build.sh

# With bestiary daemon — pooled allocation (~45 ms)
# Daemon auto-detected via Unix socket presence
$ m80 exec --profile minimal-128 --tenant-x -- ./build.sh

# Explicit daemon start (foreground or background)
$ m80-bestiary start --config /etc/m80/bestiary.toml
```

---

## What this means for m80 itself

The Bestiary lives outside m80 per CLAUDE.md scope boundary. m80's job is to
expose the right primitives so the Bestiary (or any equivalent caller) can
build this strategy:

- **Drive hot-plug** — Firecracker `PATCH /drive` support in
  `m80-firecracker-client`; guestd-side udev/mount handler.
- **Tenant-identity vsock contract** — m80-proto envelope for guestd to read
  `/workspace/.tenant-identity` and report it; host-side compare and kill on
  mismatch.
- **One-shot lifecycle flag** — m80-firecracker mode that destroys the VM
  after a single exec/stop without further reuse.
- **Snapshot restore** — already shipped (m80-rrp.3, closed).
- **Reliability under saturation** — m80-c78m must close before the
  under-load latency claims are honest.

These primitives are tracked by epic **m80-iswt**. The Bestiary itself is not
tracked in m80 beads; it is downstream work in a separate codebase.

---

## Summary

The conveyor-belt strategy works because it converts a hard correctness
problem into a simple operational one:

- **Security:** No cross-tenant leakage is possible because no VM is ever
  reused. Destruction is a stronger guarantee than cleanup.
- **Latency:** 45 ms allocation via a pre-booted generic pool. Background
  refill keeps the pool full without affecting the request path.
- **Efficiency:** RAM scales with concurrency, not tenant count. Scale-to-zero
  when idle. Different profiles coexist as snapshot files on disk.
- **Simplicity:** Each VM has one lifecycle: blank → mount → execute →
  destroy.

This is the architecture that makes multi-tenant sandboxed code execution
both fast and provably safe.
