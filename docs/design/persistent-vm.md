# Persistent VM — Design

**Bead:** `m80-qokt.2.1`
**Date:** 2026-05-04
**Status:** DESIGN — no code landed yet

---

## 1. API surface change

The current `RunningSandbox::exec` consumes `self`, forcing one VM per exec call:

```rust
// v0.1 — current
pub fn exec(self, req: ExecRequest) -> Result<ExecResponse, FcError>
```

Persistent VM relaxes this to `&mut self`:

```rust
// v0.2 — this design
pub fn exec(&mut self, req: ExecRequest) -> Result<ExecResponse, FcError>
```

Sequential, one in-flight exec at a time. The call blocks the calling thread until guestd returns the `ExecResponse`. Pipelining (concurrent exec on one VM) is not supported and not a goal.

The borrow checker enforces the sequential contract: `exec` takes `&mut self`, which prevents a second `exec` call while the first is outstanding without any runtime lock. Drop semantics on `RunningSandbox` are unchanged — the VM is stopped on drop as before; only the receiver changes.

All call sites that currently pass `sandbox.exec(req)` by value update to `sandbox.exec(req)` with `&mut sandbox` in scope. The update is mechanical (see `m80-qokt.2.2`).

---

## 2. State persistence guarantees

This IS the feature. Each `exec` call runs inside the same VM, against the same mutable state:

**Filesystem (overlay upper layer):** The per-VM `rootfs.overlay.ext4` (`/dev/vdb`) stays mounted as the overlayfs upper directory across all exec calls. Files written by exec N are visible to exec N+1. The base rootfs (`/dev/vda`, shared RO) is untouched.

**`/tmp`:** Lives on the overlay upper layer. Persists across exec calls within a session. Not cleaned between execs.

**Environment via shell history:** If the caller runs a shell and uses `source` or writes env-setting commands, those side effects accumulate in the process tree that guestd manages. Note: each `ExecRequest` spawns a fresh child process. Environment variables set inside a prior exec's process are NOT visible to the next exec's process unless the caller explicitly encodes them in `ExecRequest.env`. The persistence here is filesystem-level (e.g. a `.bashrc` written by exec N is readable by exec N+1's shell), not in-memory across process boundaries.

**Workspace scratch (`/dev/vdc`):** If `SandboxConfig::workspace` is configured, the scratch ext4 is also persistent across exec calls. All exec calls in the session share the same workspace image.

**What does NOT persist:** in-memory process state. Each `ExecRequest` spawns a new child process. There is no daemon accumulating state across execs in guestd — guestd is a dispatcher, not a REPL.

**Interaction with `extract_changes`:** `StoppedSandbox::extract_changes` operates on the scratch image after `stop()`. Persistent-VM callers that want an intermediate snapshot of overlay state must stop the VM first (no mid-session extract). This is not a new constraint; it exists in v0.1.

---

## 3. Cancellation contract

### 3.1 Design choice: Drop-based cancel guard

The host-side cancellation surface is a **drop guard**. `RunningSandbox::exec` returns an `ExecGuard` that wraps the in-flight call:

```rust
pub struct ExecGuard<'a> {
    sandbox: &'a mut RunningSandbox,
    request_id: String,
    done: bool,
}

impl<'a> ExecGuard<'a> {
    /// Block until the response arrives. Consumes the guard (sets done = true).
    pub fn wait(mut self) -> Result<ExecResponse, FcError> { ... }
}

impl<'a> Drop for ExecGuard<'a> {
    fn drop(&mut self) {
        if !self.done {
            // caller abandoned the guard without calling wait() — cancel
            self.sandbox.send_cancel(&self.request_id);
        }
    }
}
```

`RunningSandbox::exec` becomes:

```rust
pub fn exec(&mut self, req: ExecRequest) -> Result<ExecGuard<'_>, FcError>
```

**Why drop-guard, not an explicit `cancel()` method or a `CancelToken`:**

A `CancelToken` requires the caller to carry two things (the token and the future), thread the token to an out-of-band path, and is only meaningful in an async context where dropping a future is cheap. m80's exec path is synchronous and blocking. A drop guard is the idiomatic Rust mechanism for "if you stop caring, the cleanup happens automatically." It requires no caller ceremony, matches how RAII works throughout the codebase, and the borrow on `&mut self` inside the guard prevents a second exec from starting while cancellation is pending. The explicit `cancel()` alternative is redundant — the caller can call `guard.sandbox.send_cancel(...)` directly if needed, but we don't expose that as a public surface because it's the drop that's the contract.

Callers that always want the result call `guard.wait()`, which is zero overhead over a direct blocking call. Callers that want to abandon call `drop(guard)` or just let it go out of scope.

### 3.2 Wire shape

Two new envelope types added to `m80-proto::types`. Whichever epic between `m80-qokt.2` and `m80-5vha` (streaming exec) lands first owns these types; the second consumes them without re-adding:

```rust
/// Host → guest: cancel an in-flight exec.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CancelRequest {
    /// Matches the `request_id` on the `ExecRequest` envelope being cancelled.
    pub request_id: String,
}

/// Guest → host: acknowledgement that the cancel was processed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CancelAck {
    pub request_id: String,
    pub status: CancelStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CancelStatus {
    /// Guest killed the process on request.
    Cancelled,
    /// Process had already exited before the cancel arrived.
    AlreadyExited,
    /// Guest could not kill the process (e.g. waitpid failed after SIGKILL).
    Failed,
}
```

Wire `kind` constants follow the existing pattern:

```
PAYLOAD_KIND_CANCEL_REQUEST  = "cancel_request"
PAYLOAD_KIND_CANCEL_ACK      = "cancel_ack"
```

### 3.3 Sequence

1. Caller drops `ExecGuard` without calling `wait()`.
2. `ExecGuard::drop` calls `sandbox.send_cancel(request_id)`, which sends `Envelope<CancelRequest>` over the existing vsock channel and then calls `channel.recv_with_timeout::<CancelAck>(Duration::from_secs(2))`.
3. **Guestd side:** on receipt of `cancel_request`, guestd looks up the running child by `request_id`, sends `SIGKILL`, calls `wait()`, and responds with `CancelAck { status: Cancelled | AlreadyExited }`. If the kill or wait fails, it responds `CancelAck { status: Failed }`.
4. **Host side (ack received):** `send_cancel` returns. Drop completes. VM remains in `Running` state.
5. **Host side (ack timeout after 2 s):** `send_cancel` sets `self.poisoned = true` on the sandbox before returning. Subsequent `exec()` calls check this flag and return `FcError::SandboxPoisoned` immediately without sending to guestd.

The vsock channel is the same channel used for exec — it is NOT multiplexed. The cancel RPC is sequential with exec: `send_cancel` is only called from `Drop`, which only runs after the exec send completed (or partially completed). The channel is in a defined state: guestd is either executing and will read the cancel next, or it already replied (AlreadyExited path).

**guestd handler dispatch:** Add `PAYLOAD_KIND_CANCEL_REQUEST` to the `match raw.kind.as_str()` dispatch in `connection.rs`. The handler thread receives the cancel, kills the child thread's process (the child PID is shared via an `Arc<Mutex<Option<u32>>>`), and responds. The exec handler thread detects that the child is gone and abandons its response write — the channel is now owned by the cancel handler.

---

## 4. Idle timeout

```rust
pub struct SandboxConfig {
    // ... existing fields ...

    /// How long to let the VM sit idle (no exec in flight, none pending) before
    /// issuing a graceful shutdown. `None` opts out of idle shutdown entirely.
    ///
    /// Default: `Some(Duration::from_secs(300))` (5 minutes).
    pub idle_timeout: Option<Duration>,
}
```

Default `Some(5 min)` is intentional. An opted-out persistent VM (`None`) must be explicitly stopped by the caller. If a caller leaks a `RunningSandbox` without stopping it, the admission permit remains held and the VM continues running until process exit. `None` is the right choice for long-lived session daemons that manage their own lifecycle; `Some(5 min)` is the right default for agent call-and-forget usage.

**Implementation:** After each `exec` completes (or after `send_cancel` returns), the sandbox resets a monotonic deadline: `idle_deadline = Instant::now() + idle_timeout`. A background thread (spawned when the sandbox enters `Running` state, parked most of the time) wakes at the deadline. If `Instant::now() >= idle_deadline` and no exec is in flight (tracked with an `AtomicBool`), the thread calls the existing `send_shutdown_request` helper (already in `lifecycle.rs`) and then sets a flag that causes the next `exec` call to return `FcError::SandboxStopped`. The background thread does NOT consume `self` — shutdown is advisory. If `send_shutdown_request` fails (VM already gone), the thread logs a warning and exits.

Callers that want shutdown notification poll `sandbox.is_alive()` or observe `FcError::SandboxStopped` on the next `exec`.

`idle_timeout: None` — no background thread is spawned.

---

## 5. Poisoning

`RunningSandbox` gains an internal `poisoned: bool` field (not public). It is set to `true` when:

- `send_cancel` does not receive a `CancelAck` within 2 seconds.
- guestd sends `CancelAck { status: Failed }` (kill failed — process may be orphaned).
- The vsock channel returns an unrecoverable error during any send or recv (channel closed, framing error).

Once poisoned, the sandbox does not attempt further communication with guestd:

```rust
pub fn exec(&mut self, req: ExecRequest) -> Result<ExecGuard<'_>, FcError> {
    if self.poisoned {
        return Err(FcError::SandboxPoisoned);
    }
    // ... rest of exec ...
}
```

`FcError::SandboxPoisoned` is a new variant. Callers receiving it must `stop()` (which SIGKILLs the Firecracker process directly via `force_kill`) and re-launch. A poisoned sandbox can still be stopped — `stop()` routes through `bounded_stop` which tries the vsock shutdown first but falls back to SIGKILL on failure; the fallback is the expected path for a poisoned VM.

**`types.rs` change:** Add a `poisoned` field to `RunningSandbox`. This is an internal detail; the public state machine in the README describes poisoning as a terminal condition requiring re-launch, not as a separate enum state. The type remains `RunningSandbox` (not a new type) because no new methods are available on a poisoned sandbox — only `stop()` and `force_kill()` remain useful, and those already exist.

```rust
pub struct RunningSandbox {
    // ... existing fields ...
    pub(crate) poisoned: bool,
}
```

---

## 6. Interaction with reset-evidence vocabulary (`m80-rrp.1`)

Orthogonal. No cross-dep in either direction.

Warm-pool VMs (`m80-rrp.3+`) are restored from a clean snapshot, execute one logical unit of work, and are then recycled. They consume the reset-evidence vocabulary — the caller can observe what changed between the clean snapshot state and the post-exec state. That vocabulary is appropriate for stateless sessions.

Persistent VMs are the opposite choice: the caller explicitly wants state to accumulate. There is no "reset" between exec calls; the overlay disk is not snapshotted or rolled back. The reset-evidence vocabulary has no role here — there is no baseline to diff against, because persistence IS the baseline.

A future `m80-adapter` may choose to route long-running stateful sessions to a persistent VM and short stateless sessions to warm-pool VMs. That routing is above m80's layer. m80 exposes both modes; the adapter decides which to use per session.

---

## 7. Risk register

Distilled from `docs/planning/perf-roadmap-extended.md §4.1`:

| # | Failure mode | Detection | Mitigation |
|---|---|---|---|
| R1 | `&mut self` API mutation breaks every call site at once | compile fails after `m80-qokt.2.2` lands | Sequence: DESIGN → API mutation → consumers in same diff. Pre-1.0 internal API; no shims. |
| R2 | Sequential exec accumulates state — turn N sees turn N-1's leftovers | unintended side-effects for callers expecting clean sessions | Feature, not bug, when caller wants persistence. Document: callers wanting clean-slate use warm-pool VMs (`m80-rrp.4+`), not persistent VMs. |
| R3 | Cancellation contract during in-flight exec is ambiguous | guest process becomes orphan; future exec sees zombies | Drop-guard cancel contract (this doc §3). 2 s ack timeout → Poisoned → re-launch. |
| R4 | Idle persistent VM leaks resources — no timeout means N stale VMs holding admission permits | host RAM exhausted; semaphore starved | `SandboxConfig::idle_timeout` default `Some(5 min)`. Background thread issues shutdown on expiry. |
| R5 | Turn-to-turn latency is not ~0 due to vsock contention or guestd state thrashing | BENCH P50 of turn N>1 still >50 ms | Investigate at `m80-vsock` level; not a launch-latency issue. Mitigation is out of scope for this epic. |
| R6 | A broken exec leaves the VM in a bad state with no caller signal | next exec returns garbage | Poisoning mechanic (this doc §5). Unrecoverable channel errors set `poisoned = true`; next exec returns `SandboxPoisoned`. |

---

## 8. Bench expectation

**Mechanism:** Persistent VM eliminates the full boot sequence for every exec call after the first. In the current baseline, cold launch costs ~1.9 s (minimal image, unoptimized storage). Post-chain (storage pivot + stripped kernel + snapshot/restore), cold launch costs ~200 ms. Turn-to-turn latency on a persistent VM is the vsock round-trip cost only — low single-digit milliseconds for short commands. The per-turn saving is roughly equal to the cold-launch cost that would otherwise be paid.

**Confidence: high on mechanism.** The saving is structural — we are removing the boot phase entirely, not optimizing it. There is no measurement uncertainty on whether boot is skipped; it is, by construction.

**Confidence: LOW on net-positive impact at workload level** if warm-pool restore (`m80-rrp.3+`) is also live. Warm-pool restore already drives warm-path launch to ~200 ms from a pre-restored VM. If turn-to-turn latency for a warm-pool session is 200 ms (restore + exec) and for a persistent-VM session is ~5 ms (exec only), the absolute difference is 195 ms per turn. For a 10-turn session: persistent VM saves ~1.75 s total over the whole session vs warm pool. Whether that matters depends on the agent's total work budget per turn. For a 2 s tool call, 195 ms is ~10% overhead. For a 200 ms tool call, it is the dominant cost.

**When persistent VM wins clearly:** stateful sessions where warm-pool VMs are wrong — sessions that require filesystem continuity, environment accumulation, or long-running background processes inside the guest. A warm-pool VM cannot provide these; persistent VM is the only correct tool.

**When warm-pool wins:** clean stateless tool calls where agent latency is dominated by the exec itself, not the boot. In this regime both modes have similar latency and warm pool's fresh state is safer (no leftover files from prior turns).

**The bench (m80-qokt.2.6) is the source of truth.** Report P50 of N=30 sequential execs on one VM vs N=30 cold launches and N=30 warm-pool restores. The three-way comparison makes the nuance concrete.

---

## Cross-references for lead sanity-check

- **`m80-5vha` (streaming exec epic):** shares the `CancelRequest` / `CancelAck` / `CancelStatus` envelope types defined in §3.2. Whichever epic lands first adds these to `m80-proto::types`; the second imports without re-declaring. Lead should verify the two epics have not defined conflicting shapes before either IMPL lands.

- **`m80-rrp.1` (reset-evidence vocabulary):** confirmed orthogonal (§6). No code dep; design contact only.

- **`m80-rrp.3` / `m80-rrp.4` (warm pool):** the bench comparison in §8 requires warm-pool numbers. `m80-qokt.2.6` BENCH should run after `m80-rrp.3` BENCH exists so the three-way comparison is possible. Dependency is soft — the bench can publish a two-way comparison (persistent vs cold) and be updated to three-way when warm pool lands.

- **`m80-firecracker` README:** the "Non-goals" section currently says "No persistent VM pools. Every `launch()` is a fresh boot in v0.1; warm pools are a v0.2 epic." When `m80-qokt.2.2` lands, update to reflect multi-exec on `RunningSandbox`. This README update is part of `m80-qokt.2.2`'s acceptance, not this DESIGN leaf.
