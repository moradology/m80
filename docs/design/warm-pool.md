# Warm Pool

## Contract

The warm pool owns pre-restored, guestd-ready `RunningSandbox` slots and
hands out one clean slot per allocation. It is a request-path latency
tool: callers pay `try_lease` plus the first `exec`, while snapshot
restore happens before demand or in a background refill after a slot is
leased or discarded.

Measured baselines that shape the design:

- Persistent sequential exec: about 20.6 ms P50/P95 when the caller wants
  state continuity.
- Direct snapshot restore: 274.204 ms P50 / 280.132 ms P95 idle,
  444.972 ms P50 / 588.221 ms P95 loaded.
- Cold launch: still seconds-scale; stripped kernel trims only about
  100 ms, so clean stateless calls need allocation from a ready slot
  rather than further cold-boot trimming.

Use persistent `RunningSandbox` sessions for stateful workflows. Use
direct `Sandbox::launch_from_snapshot` when the caller wants explicit
restore ownership. Use `WarmPool` when the caller needs a clean
stateless VM on the request path.

## State Machine

Slot states:

- `Filling`: a restore from the configured snapshot is in progress.
- `Ready`: the restored VM has passed the restore exec-channel probe and
  is available for `try_lease`.
- `Leased`: a caller owns the slot through `WarmLease`.
- `Discarded`: the lease was released or dropped and the VM is killed and
  deleted.

Transitions:

- Pool fill starts `Filling`.
- Successful restore plus successful `ready_probe` exec moves
  `Filling -> Ready`. The ready probe is the publication gate; there is
  no fixed post-probe sleep in the warm-pool hot path.
- Restore failure records `last_fill_error` and removes the filling slot.
- `try_lease` moves `Ready -> Leased` and starts background refill.
- `WarmLease::discard` or `Drop` moves `Leased -> Discarded` and starts
  background refill after teardown.

Empty-pool behavior is fail-closed. `try_lease` returns
`FcError::PoolEmpty`; it never hides a cold boot or synchronous restore
fallback inside allocation.

## Reset Rule

Reuse requires explicit `BlankVmResetEvidence`:

1. Ownership marker and lease are current.
2. Boot identity matches the clean template.
3. No workspace id is attached.
4. No run id is attached.
5. Guest workspace is empty or template-equal.
6. Run-root surface is clean.
7. Diagnostics are clean.
8. Post-reset guestd probe succeeds.

Any missing, stale, ambiguous, or failed input means
`BlankVmResetDecision::Discard`. The first implementation does not
attempt reset, so every lease defaults to discard with
`BlankVmResetDiscardReason::ResetEvidenceUnavailable`.

The pool must not infer reuse from liveness, process handles, socket
existence, metrics presence, or a clean-looking directory tree.

## Sizing

Required admission headroom is:

```text
max_concurrent_vms >= target_ready + max_simultaneous_leases + max_filling
```

`target_ready` should cover burst demand during the restore tail. A
starting estimate is:

```text
target_ready = ceil(request_rate_per_second * restore_p95_seconds) + burst_margin
```

Run-root capacity must cover each ready, leased, and filling slot:

```text
slot_bytes ~= snapshot_mem_bytes + sparse_overlay_growth + console/log overhead
```

Because refill and the ready probe are background work, loaded restore
variance affects how quickly the ready reserve recovers, not the
allocation cost for a slot that was already ready.
