# Request ID Correlation

Behavior capture for `m80-9fcy`.

## Present-Tense Behavior

`m80 run` mints one opaque ULID-based request id at command entry. The id is
formatted with a `req_` prefix and has no semantic meaning beyond correlation.

The same id is carried through:

- plain wrapper stderr: `error: [req_...] ...`
- JSON output envelopes for run-scoped commands
- JSON wrapper error payloads
- `SandboxConfig::request_id`
- `<run_dir>/diagnostics.jsonl` host lifecycle and request events
- `Envelope::request_id` for buffered exec, streaming exec, PTY exec, and
  cancellation frames
- warm owner `Run` / `RunStream` control messages and warm run/error responses

When no caller request id exists, `m80-firecracker` generates a per-request id
from the VM id, request kind, and monotonic time. That fallback is for direct
library callers only; the CLI path always supplies one.

## Boundary

The id is opaque. It is not a workspace id, tool-call id, correlation id,
idempotency key, or agent semantic carrier. Higher-level adapters may map their
own ids to the m80 request id, but m80 core does not interpret it.

## Verification

- `crates/m80-cli/tests/output_error_contract.rs` and
  `crates/m80-cli/tests/feature_gap_smoke.rs` pin request ids on wrapper
  errors and JSON envelopes.
- `crates/m80-firecracker/src/lifecycle/exec.rs` tests configured request id
  selection and fallback generation.
- `crates/m80-firecracker/tests/diagnostics_log.rs` pins diagnostics JSONL
  request-id shape.
- Existing `m80-guestd` request-id tests pin guest echo/log behavior once the
  host supplies `Envelope::request_id`.
