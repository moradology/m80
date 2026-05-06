# Guest Exec — Cancellation Behaviors

## disconnect-kills-child

If the host disconnects the vsock connection while a streaming child is
running, the daemon kills and reaps the child and emits no `ExecExit`.
Streaming mode has two detection paths: write-side failure while sending a
chunk, and read-side EOF while the child is still running. The read-side path
is required for silent commands such as `sleep 600`.

Buffered mode still writes one response at process exit or timeout. If the
host has gone away by then, the response write fails and the daemon returns to
the accept loop after reaping the child.

Source: dossier `03-guest-daemon.md` § Process orchestration; predecessor `services/guestd-rs/src/main.rs:318-329` (per-connection error path drops stream and reaps).

Tests: `m80-guestd/tests/streaming_exec.rs::streaming_reader_eof_kills_silent_child_without_exit_frame`
and `m80-guestd/tests/streaming_exec.rs::streaming_chunk_write_failure_kills_child_promptly`.

## partial-flush

On buffered timeout, the daemon returns whatever stdout and stderr bytes were
collected before the event. In streaming mode, bytes already written as chunks
stay written; explicit cancel returns `CancelAck`, and disconnect cancellation
returns no terminal frame.

Source: dossier `03-guest-daemon.md` § Process orchestration; predecessor `crates/sandbox/agent-sandbox-local/src/executor.rs`.

Test: `m80-guestd/tests/handle_connection.rs::exec_with_timeout_returns_timed_out` — verifies that a timed-out response is still returned (partial output flushed, even if empty for `sleep 60`).
