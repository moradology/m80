# Diagnostics Log

Behavior capture for `m80-1f8.1` and `m80-9fcy`.

## JSONL Format

Each VM run directory gets `<run_dir>/diagnostics.jsonl` when the diagnostics
writer can be opened. The file is append-only JSONL. Each line is one
`VmEvent`:

```json
{
  "schema_version": 2,
  "timestamp_unix_ms": 1777777777777,
  "source_class": "host",
  "phase": "Request",
  "message": "exec request started",
  "request_id": "req_01J...",
  "context": {
    "vm_id": "vm-123"
  }
}
```

The `request_id` is opaque. m80 uses it only for correlating CLI output, host
events, guest stderr, and wire frames. It is not a `workspace_id`,
`tool_call_id`, `correlation_id`, or idempotency key.

## Phase Enum

The diagnostics phase vocabulary is:

- `StartupScavenge`
- `HostPreflight`
- `StoragePrepare`
- `NetworkPrepare`
- `Boot`
- `Ready`
- `Request`
- `Stop`
- `Writeback`
- `Delete`

## Option Wrapper

`m80-firecracker` carries diagnostics as `Option<Diagnostics>`. If the writer
cannot be opened or a write fails, the VM lifecycle continues and the failure
is logged through `tracing`. Diagnostics improve triage; they are not boot,
exec, stop, or delete authority.

## Verification

- `crates/m80-observability/src/lib.rs` tests JSONL schema version, request id,
  context, disabled no-op behavior, and phase serialization.
- `crates/m80-observability/tests/v01_surface.rs` verifies the public event
  type round-trips through serde.
- `crates/m80-firecracker/tests/diagnostics_log.rs` verifies the firecracker
  diagnostics-facing JSONL fixture shape.
- `crates/m80-firecracker` records launch, request, stop, and delete events
  through the optional diagnostics handle.
