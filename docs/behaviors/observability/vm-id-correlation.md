# VM ID Correlation

Behavior capture for `m80-9jfaz.2`.

## Contract

Every per-VM launch, restore, exec, capture, stop, force-kill, warm-lease exec,
and teardown path owns the selected VM id. These public entrypoints open a
`tracing` span with structured `vm_id` so nested lifecycle events are
correlatable by subscribers.

Best-effort teardown warnings that can happen after the normal owner frame has
unwound also carry `vm_id` directly on the event. This includes snapshot bind
unmount cleanup, graceful-stop fallback, force-kill leak preservation, exec and
PTY forwarder detach failures, hotplug discard cleanup, one-shot warm-lease
discard cleanup, warm-lease Drop cleanup, surplus/unleased warm-pool slot
discard cleanup, warm-pool shutdown cleanup, template-build discard cleanup,
jail bind teardown, placeholder-file cleanup, created-dir cleanup, and cgroup
kill/rmdir cleanup.

`MaterializedJail` and `m80_cgroup::Subtree` store the VM id when they are
created so their `Drop` implementations do not need to infer it from paths.
The VM id remains opaque VM mechanics state; it is not an adapter correlation
id, workspace id, idempotency key, or tool-call id.

## Verification

- `crates/m80-firecracker/tests/observability/vm_id_correlation.rs`
- `crates/m80-jailer/tests/observability/vm_id_correlation.rs`
- `crates/m80-cgroup/tests/observability/vm_id_correlation.rs`
