# Idle Timeout

**Bead:** `m80-qokt.2.5`
**Design ref:** `docs/design/persistent-vm.md §4`
**Tests:** `crates/m80-firecracker/tests/idle_timeout.rs`

---

## Overview

A persistent `RunningSandbox` can sit idle indefinitely between exec calls. Left
unchecked this leaks the admission permit (one slot in the semaphore) and keeps
the Firecracker process consuming host RAM with no benefit. The idle timeout
closes this hole: after a configurable period of inactivity the host issues a
graceful shutdown and marks the sandbox as expired.

---

## Configuration

`SandboxConfig::idle_timeout: Option<Duration>` controls the timeout.

| Value | Behavior |
|---|---|
| `Some(d)` | Watcher thread spawned; VM shut down after `d` of inactivity. |
| `None` | No watcher spawned; VM runs until explicitly stopped. |

Default: `Some(Duration::from_secs(300))` (5 minutes). The default is
intentional — a leaked `RunningSandbox` from a caller that dropped it without
`stop()` will still be reclaimed within 5 minutes.

**Test:** `idle_timeout_default_is_five_minutes` — unit; verifies
`SandboxConfig::default().idle_timeout == Some(300s)`.

**Test:** `idle_timeout_none_disables_watcher` — unit; verifies `None` is
accepted and round-trips without panicking.

---

## Watcher thread

When `idle_timeout` is `Some(d)`, `Sandbox::launch` (and
`Sandbox::launch_from_snapshot`) spawn a background thread immediately after
the sandbox enters the `Running` state. The thread:

1. Sleeps for `min(timeout/4, 30s)` between checks to avoid burning CPU while
   remaining responsive.
2. Suppresses expiry while `active_execs` is non-zero. This distinguishes a
   silent in-flight command from an actually idle VM.
3. Loads `last_activity_ns` (an `Arc<AtomicU64>` shared with `exec`) and
   compares it to `monotonic_ns()`. If `now - last_activity >= timeout`, the
   watcher fires.
4. On firing: sets `idle_timed_out` (`Arc<AtomicBool>`) to `true`, then calls
   `send_shutdown_request` over the vsock channel (best-effort; logs a warning
   if the VM is already gone) and exits.
5. Exits cleanly when `watcher_stop` is set (by `stop()` or `force_kill()`).

The watcher does not consume `self` and cannot call `stop()` (which requires
ownership). The caller observes expiry through `FcError::IdleTimedOut` on the
next `exec` call — see "Exec gate" below.

**Test:** `watcher_fires_after_inactivity` — unit; simulates the watcher loop
directly with a stale `last_activity_ns` value and verifies `idle_timed_out`
is set.

**Test:** `watcher_does_not_fire_when_activity_reset` — unit; verifies that
regularly resetting `last_activity_ns` prevents the watcher from firing.

**Test:** `idle_watcher_does_not_fire_while_exec_is_in_flight` — unit; drives
the actual watcher loop with stale activity and `active_execs = 1`, then proves
the watcher does not mark the sandbox idle.

---

## Exec gate

`RunningSandbox::exec` checks `idle_timed_out` at entry:

```rust
if self.idle_timed_out.load(Ordering::Relaxed) {
    return Err(FcError::IdleTimedOut);
}
```

On entry, `exec` creates an activity guard that increments `active_execs` and
touches `last_activity_ns`. Dropping that guard decrements `active_execs` and
touches `last_activity_ns` again. This ensures:

- A long-running exec does not expire mid-flight (the deadline is extended for
  the duration of the exec).
- The idle clock starts from completion, not from when the caller queued the
  next exec.

**Test:** `idle_timeout_resets_on_exec` — KVM-gated `#[ignore]`; launches with
`idle_timeout: Some(2s)`, execs twice within the window (both succeed), then
sleeps past the timeout and verifies the third exec returns `IdleTimedOut`.

**Test:** `idle_timeout_fires_after_inactivity` — KVM-gated `#[ignore]`;
launches with `idle_timeout: Some(2s)`, does not exec, sleeps 3 s, and verifies
the first exec returns `IdleTimedOut`.

---

## Stop / force_kill interaction

`RunningSandbox::stop` and `force_kill` both:

1. Set `watcher_stop = true` before sending the shutdown request. This prevents
   the watcher from racing with the explicit shutdown.
2. Join the watcher thread after destructuring the sandbox (so the join happens
   after the Firecracker process is already dead or SIGKILL'd).

The join is non-blocking in practice: the watcher is sleeping on its poll
interval and the `stop_flag` check runs on the next wake. The poll interval is
at most 30 s, but in typical usage the watcher exits well before then.

---

## Error model

`FcError::IdleTimedOut` is a new variant on the `FcError` sum type. It has a
non-empty `Display` ("sandbox idle timeout expired"). The CLI maps it to
`EXIT_IDLE_TIMED_OUT` (exit code 11) in `m80-cli::errors`.

**Test:** `idle_timed_out_error_displays` — unit; verifies the variant's
`Display` is non-empty.

---

## Non-goals

- **Not poisoning.** `IdleTimedOut` is a clean, expected lifecycle event —
  not an error in the vsock protocol or guestd communication. The two concepts
  are orthogonal; see `docs/design/persistent-vm.md §5` for the poisoning
  mechanic.
- **Not observable from outside exec.** There is no `is_alive()` poll method.
  Callers observe the timeout only when they next call `exec`. This is the
  simplest surface; a poll method can be added later if needed.
- **Not a replacement for explicit `stop()`.** Callers with well-defined
  session lifetimes should call `stop()`. The idle timeout is a safety net for
  callers that drop a `RunningSandbox` without stopping it.
