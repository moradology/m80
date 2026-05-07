# One-Shot Lifecycle

Captured by bead `m80-iswt.8`.

`SandboxConfig::one_shot` is a destroy-after-use lifecycle mode for
conveyor-belt callers. It defaults to `false`. When set, the first user exec or
PTY request marks the `RunningSandbox` consumed before the guest request is
sent. Later exec attempts on the same running handle fail with
`FcError::OneShotConsumed`; the caller must stop, drop, or discard the VM
instead of reusing it.

Warm-pool ready probes are not tenant workloads and do not consume the one-shot
token. A restored one-shot slot remains available for the first leased workload
after the probe succeeds.

`WarmLease` turns one-shot mode into automatic cleanup. If the leased sandbox is
one-shot, `exec`, `exec_with_request_id`, `exec_streaming`, and
`exec_streaming_with_request_id` take ownership of the sandbox, run exactly one
workload, force-kill/delete the VM afterward, release the lease, and trigger
background refill. Cleanup happens after both successful and failed exec
attempts. If cleanup fails after a successful workload, the cleanup error is
returned so the caller does not observe a reusable-clean success.

One-shot does not make file operations, metrics, or hotplug attach count as the
tenant workload. Those operations prepare or inspect the VM; the user workload
boundary remains exec/PTY.

Evidence:

- `crates/m80-firecracker/src/lifecycle/exec.rs` claims the one-shot token at
  user exec/PTY entry and returns `FcError::OneShotConsumed` after it is used.
- `crates/m80-firecracker/src/warm_pool.rs` suppresses one-shot consumption for
  ready probes and discards one-shot leases immediately after the first exec.
- Unit tests in `crates/m80-firecracker/src/lifecycle/exec.rs` pin the token
  semantics.
