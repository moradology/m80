# Guest Exec — Cancellation Behaviors

## disconnect-kills-child

If the host disconnects the vsock connection while a child is running, the daemon detects the broken stream (write_frame returns an I/O error) and the child is cleaned up. In m80-guestd v0.1 the daemon processes one connection sequentially: the child runs to completion (or timeout) before the response is written, so a disconnect mid-exec means the response write fails silently. The child is not orphaned — wait() is always called.

Source: dossier `03-guest-daemon.md` § Process orchestration; predecessor `services/guestd-rs/src/main.rs:318-329` (per-connection error path drops stream and reaps).

Note: v0.1 does not explicitly watch for connection close during execution. Full mid-exec cancellation (SIGKILL on disconnect) is a v0.2 item. The current design ensures no zombie children by always calling `wait()` after the timeout path.

## partial-flush

On cancellation (or any early failure), the daemon returns whatever stdout and stderr bytes were collected before the event. The response buffers are always populated from the capture threads before the response is written.

Source: dossier `03-guest-daemon.md` § Process orchestration; predecessor `crates/sandbox/agent-sandbox-local/src/executor.rs`.

Test: `m80-guestd/tests/handle_connection.rs::exec_with_timeout_returns_timed_out` — verifies that a timed-out response is still returned (partial output flushed, even if empty for `sleep 60`).
