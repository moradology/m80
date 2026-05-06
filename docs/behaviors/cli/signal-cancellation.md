# CLI Signal And Cancellation

Behavior capture for bead `m80-lt15.22`.

## Contract

`m80 run` is a foreground process wrapper. Once the guest exec channel is
established, host-side SIGINT, SIGTERM, and SIGHUP request cancellation of the
in-flight guest child instead of leaving the child running behind the wrapper.

The wrapper still owns sandbox cleanup: after cancellation completes, it stops
and deletes the sandbox by the same policy used for normal pipe-mode exits.
There is no hidden daemon, no detached process registry, and no silent fallback
to a different run mode.

## Signal Mapping

The first observed host signal wins:

| Host signal | Guest action | `m80` process exit |
|---|---|---|
| SIGINT | send `cancel_request` for the in-flight exec | 130 |
| SIGTERM | send `cancel_request` for the in-flight exec | 143 |
| SIGHUP | send `cancel_request` for the in-flight exec | 129 |

These are the conventional `128 + signal_number` process exit codes. They are
used only when the guest exec terminal status is `Cancelled`. If a signal races
with a guest process that already exited and guestd reports the normal
`ExecExit`, the guest exit code wins.

## Wire And Request Id

The host sends cancellation over the same vsock connection as the exec request.
A second connection would wait behind the in-flight request because guestd
serializes connections in v0.1.

For cancellable exec calls, `m80-firecracker` attaches an opaque request id to
the `Envelope<ExecRequest>`. The signal path sends:

```text
CancelRequest { request_id }
```

Guestd replies with `CancelAck`. If the status is `Cancelled`,
`m80-firecracker` returns a host-observed `ExecStatus::Cancelled` terminal
result. If the status is `AlreadyExited`, the host keeps waiting for the normal
`ExecExit`. If the status is `Failed`, the wrapper treats it as a wrapper
failure and force-kills the sandbox.

The request id is opaque. It is for wire pairing and diagnostics correlation,
not an agent semantic id.

## Pipe Mode

In normal pipe mode, stdout and stderr that were already received before the
signal remain byte-for-byte on host stdout/stderr. The wrapper does not add a
human cancellation banner to stdout or stderr on successful cancellation.

After cancellation, `m80 run` stops and deletes the sandbox. If stop/delete
itself fails, that failure is rendered by the normal wrapper-error path.

## JSON Mode

With global `--json`, `m80 run` uses the same cancellable exec path but buffers
the streamed output into the JSON `ExecResponse` shape. A cancelled run renders
a successful JSON response with `status: "cancelled"` and exits with the
conventional signal exit code for the first observed host signal.

Wrapper failures before or after cancellation still render versioned JSON error
envelopes on stderr through the normal error contract.

## PTY Mode

PTY mode will reuse this cancellation base. Terminal-generated Ctrl-C is guest
terminal input, not a wrapper signal, while wrapper-level SIGTERM/SIGHUP/drop
still cancel the guest foreground process and clean up the sandbox. Resize and
terminal raw-mode restoration remain owned by the PTY beads.

## Process Group Semantics

Guestd starts pipe-mode exec children in a new process group. Cancellation,
timeout, host disconnect/read EOF, and streaming write failure all target that
process group, not only the direct child.

The termination order is:

1. send SIGTERM to the process group
2. wait a bounded 100 ms grace period
3. send SIGKILL to the same process group
4. reap the direct child before returning to the accept loop

PTY mode must reuse this process-group/session rule when its runner lands; it
must not regress to direct-child-only cancellation.

## Verification

- `crates/m80-cli/src/cmds/tests.rs::cancelled_run_maps_to_conventional_signal_exit_code`
- `crates/m80-cli/src/cmds/tests.rs::non_cancelled_run_preserves_guest_exit_code_even_if_signal_raced_late`
- `crates/m80-cli/tests/e2e_run_passthrough.rs::sigterm_cancels_guest_child_and_deletes_sandbox`
  is an ignored real-KVM smoke test.
- `crates/m80-vsock/tests/frame_round_trip.rs::cloned_sender_writes_control_frame_on_same_connection`
- `crates/m80-firecracker/src/lifecycle/exec.rs::tests::cancelled_exit_reports_host_observed_cancel_status`
- `crates/m80-firecracker/tests/streaming_exec.rs::cancellable_streaming_exec_kills_shell_grandchild_and_allows_next_exec`
  is an ignored real-KVM smoke test.
- `crates/m80-guestd/tests/handle_connection.rs::cancel_mid_exec_kills_shell_spawned_grandchild`
- `crates/m80-guestd/tests/handle_connection.rs::timeout_kills_shell_spawned_grandchild`
- `crates/m80-guestd/tests/streaming_exec.rs::streaming_cancel_request_kills_shell_spawned_grandchild`
- `crates/m80-guestd/tests/streaming_exec.rs::streaming_reader_eof_kills_shell_spawned_grandchild_promptly`
- `crates/m80-guestd/tests/streaming_exec.rs::streaming_timeout_kills_shell_spawned_grandchild`
