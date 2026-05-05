# Exec cancellation

**Bead:** `m80-qokt.2.4`
**Status:** IMPLEMENTED

---

## Wire shape

Two new envelope types in `m80_proto::types` (live since this bead):

```
cancel_request  — host → guest
cancel_ack      — guest → host
```

Wire `kind` constants: `PAYLOAD_KIND_CANCEL_REQUEST = "cancel_request"`,
`PAYLOAD_KIND_CANCEL_ACK = "cancel_ack"`.

```rust
pub struct CancelRequest {
    pub request_id: String,   // must match the Envelope::request_id of the in-flight exec
}

pub struct CancelAck {
    pub request_id: String,   // echoed from CancelRequest
    pub status: CancelStatus,
}

pub enum CancelStatus {
    Cancelled,     // process was running; SIGKILL sent and child reaped
    AlreadyExited, // process had already exited, or request_id did not match
    Failed,        // SIGKILL itself failed (rare; kill(2) returned an unexpected error)
}
```

`CancelStatus` wire values (serde `snake_case`): `cancelled`, `already_exited`, `failed`.

---

## Semantics

### Channel ownership

The cancel is sent over the **same vsock channel** as the exec — the channel is not multiplexed. The host sends `cancel_request` on the connection that already sent `exec_request`, before reading the `ExecResponse`. Guestd reads both frames from the same connection: it processes the exec frame first (spawning the child), then picks up the cancel frame in the poll loop and SIGKILLs.

### Guestd dispatch (connection.rs)

When an `exec_request` arrives, guestd spawns the child process in a background thread and enters a **two-path poll loop**:

1. `child_rx.try_recv()` — checks whether the exec thread has finished.
2. `reader.fill_buf()` + `read_frame` — reads the next incoming frame (non-blocking peek).

If a `cancel_request` frame arrives **before the child exits**:
- The handler signals the exec thread via an `mpsc::channel`.
- It spins until the child PID is published in `Arc<Mutex<Option<u32>>>` (populated by the exec thread immediately after `Command::spawn`).
- Sends `SIGKILL` via `nix::sys::signal::kill`.
- Waits for the exec thread to confirm the reap.
- Writes `CancelAck { status: Cancelled }` to the wire.
- Returns `ConnectionOutcome::Continue`.

If the `request_id` in `cancel_request` does NOT match the in-flight exec:
- Writes `CancelAck { status: AlreadyExited }` immediately.
- Continues waiting for the child to finish naturally (or time out).
- Then writes the `ExecResponse` as normal.

If a `cancel_request` arrives when **no exec is in flight** (dispatch-level handler):
- Writes `CancelAck { status: AlreadyExited }` immediately.
- Returns `ConnectionOutcome::Continue`.

If `kill(2)` itself fails with any errno other than `ESRCH` (which maps to `AlreadyExited`):
- Writes `CancelAck { status: Failed }`.

### 2-second ack timeout (host side — Wave 4)

The Drop-guard (`ExecGuard::drop`) calls `send_cancel` and waits up to 2 seconds for `CancelAck`. If the ack does not arrive, the sandbox is marked `Poisoned`. That host-side state machine is implemented in `m80-qokt.2.5`.

---

## Status outcomes

| `CancelStatus` | Condition |
|---|---|
| `Cancelled` | Process was in-flight; `SIGKILL` sent; child reaped |
| `AlreadyExited` | Process exited before cancel arrived, OR `request_id` mismatch, OR no exec in flight |
| `Failed` | `kill(2)` returned an unexpected error (not `ESRCH`) |

---

## Cross-reference: m80-5vha (streaming exec)

The `CancelRequest`, `CancelAck`, and `CancelStatus` types are designed to be shared with the streaming-exec epic (`m80-5vha`). This bead landed first; `m80-5vha` will import these types from `m80_proto::types` without re-declaring them. See `docs/design/persistent-vm.md §3.2`.

---

## Tests

### Unit tests (no VM required)

File: `crates/m80-guestd/tests/handle_connection.rs`

| Test | Behavior pinned |
|---|---|
| `cancel_mid_exec_returns_cancelled_ack` | Cancel during `sleep 60` → `CancelAck { Cancelled }` |
| `cancel_mid_exec_terminates_quickly` | Same scenario completes in < 5 s (not 60 s) |
| `cancel_wrong_request_id_returns_already_exited` | Mismatched `request_id` → `CancelAck { AlreadyExited }` |
| `cancel_no_exec_in_flight_returns_already_exited` | Standalone cancel (no exec) → `CancelAck { AlreadyExited }` |

### Serde tests

File: `crates/m80-proto/src/types.rs` (inline tests)

| Test | Behavior pinned |
|---|---|
| `cancel_request_round_trip` | `CancelRequest` serializes with `kind = cancel_request` |
| `cancel_ack_cancelled_round_trip` | `CancelAck { Cancelled }` round-trips |
| `cancel_ack_already_exited_round_trip` | `CancelAck { AlreadyExited }` round-trips |
| `cancel_ack_failed_round_trip` | `CancelAck { Failed }` round-trips |
| `cancel_status_wire_values` | `snake_case` wire representation of all three variants |

### Real-KVM integration tests (requires KVM host)

File: `crates/m80-firecracker/tests/cancellation.rs`

All are `#[ignore]`; run with `sudo cargo test -p m80-firecracker -- --ignored cancel_`.

| Test | Behavior pinned |
|---|---|
| `cancel_kills_running_process` | Full VM: `sleep 60` + cancel → `CancelAck { Cancelled }`, elapsed < 10 s |
| `cancel_after_exit_returns_already_exited` | Full VM: exec `/bin/true` → complete → cancel → `CancelAck { AlreadyExited }` |
| `wrong_request_id_returns_already_exited` | Full VM: exec `sleep 60` + cancel with bogus ID → `CancelAck { AlreadyExited }` |
