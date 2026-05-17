# Disconnect Cause Classification

## Behavior

Host receive paths that lose the peer before the required terminal response
return:

```text
FcError::Protocol(WireProtocolError::DisconnectBeforeTerminal {
    context,
    cause,
})
```

`context` names the request path that was awaiting the terminal frame.
`cause` is the host-visible classification after a short confirmation window:

- `FcProcessDead` when the recorded Firecracker PID is no longer live, including
  zombie/dead proc states waiting to be reaped.
- `UdsConnectFailed` when initial UDS open/send fails while Firecracker still
  appears live.
- `MidStreamEof` when an established stream reaches EOF before its terminal
  frame while Firecracker still appears live.
- `CleanRequestedClose` when EOF follows a host-requested graceful close and
  Firecracker still appears live.

New exec and PTY requests fail before the UDS retry loop when the recorded
Firecracker PID is already gone:

```text
FcError::SandboxDead { vm_id, firecracker_pid }
```

This keeps non-one-shot callers from spending 25 x 100 ms retrying a sandbox
known to be dead.

## Evidence

- `crates/m80-firecracker/src/lifecycle/protocol.rs` unit tests pin
  `FcProcessDead`, `UdsConnectFailed`, `MidStreamEof`, `CleanRequestedClose`,
  and `SandboxDead` classification.
- `crates/m80-firecracker/tests/streaming_exec.rs::disconnect_mid_streaming_exec_maps_to_disconnect_before_terminal`
  kills Firecracker mid-stream and asserts `DisconnectCause::FcProcessDead`.
- `crates/m80-firecracker/tests/streaming_exec.rs::exec_on_dead_firecracker_fails_fast_without_open_retry`
  kills Firecracker before a new exec and asserts `FcError::SandboxDead` within
  the fast-path budget.
- `crates/m80-firecracker/tests/malicious/truncated_frame.rs::truncated_frame_returns_disconnect_before_terminal_without_stuck_reader`
  pins the established-channel under-read case as
  `DisconnectCause::MidStreamEof`.
