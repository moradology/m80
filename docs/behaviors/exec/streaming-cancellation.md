# Streaming Exec Cancellation

Streaming exec reuses the existing `cancel_request` / `cancel_ack` envelope
types. It does not add a second cancellation protocol.

Explicit cancel:

1. Host sends `CancelRequest { request_id }` on the same connection.
2. Guestd sends SIGTERM, then bounded SIGKILL, to the child process group.
3. Guestd writes `CancelAck`.
4. Guestd writes no `ExecExit` for that request.

`m80-firecracker` exposes this to callers through
`RunningSandbox::exec_with_cancel` and
`RunningSandbox::exec_streaming_with_cancel`. The host side clones a
write-only sender for the same `Channel`; it does not open a second vsock
connection.

Disconnect cancel:

1. If writing a chunk fails, guestd terminates the child process group.
2. If a read-side EOF is observed while the child is running, guestd terminates
   the child process group.
3. Guestd writes no `ExecExit`.

The read-side EOF path is required for silent commands. A command like
`sleep 600` might never produce a chunk, so write-side `EPIPE` is not enough.

Tests:

- `crates/m80-guestd/tests/streaming_exec.rs::streaming_cancel_request_kills_child_and_returns_ack`
- `crates/m80-guestd/tests/streaming_exec.rs::streaming_cancel_request_kills_shell_spawned_grandchild`
- `crates/m80-guestd/tests/streaming_exec.rs::streaming_reader_eof_kills_silent_child_without_exit_frame`
- `crates/m80-guestd/tests/streaming_exec.rs::streaming_reader_eof_kills_shell_spawned_grandchild_promptly`
- `crates/m80-guestd/tests/streaming_exec.rs::streaming_chunk_write_failure_kills_child_promptly`
- `crates/m80-guestd/tests/streaming_exec.rs::streaming_timeout_kills_shell_spawned_grandchild`
- `crates/m80-vsock/tests/frame_round_trip.rs::cloned_sender_writes_control_frame_on_same_connection`
- `crates/m80-firecracker/src/lifecycle/exec.rs::tests::cancelled_exit_reports_host_observed_cancel_status`
- `crates/m80-firecracker/tests/streaming_exec.rs::cancellable_streaming_exec_kills_shell_grandchild_and_allows_next_exec` (ignored real-KVM smoke)
- `crates/m80-firecracker/tests/streaming_exec.rs::dropped_streaming_caller_releases_guestd_for_next_exec` (ignored real-KVM smoke)
