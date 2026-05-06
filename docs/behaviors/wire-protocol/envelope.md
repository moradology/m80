# Wire Protocol — Envelope

Behaviors captured by bead epic `m80-g3x`, leaves `m80-g3x.1.1` through
`m80-g3x.1.3`.

---

## version-field

Every request and response envelope carries an explicit `version: u32` field
stamped to `PROTOCOL_VERSION` so peers can fail-closed on mismatch without
parsing the rest of the payload.

**Present-tense statement:** The `Envelope<T>` type includes `version: u32`.
`write_frame` stamps it to `PROTOCOL_VERSION`. `read_frame` extracts `version`
via a partial parse *before* deserializing the full payload, and returns
`ProtoError::IncompatibleVersion` if the value does not equal `PROTOCOL_VERSION`.

**predecessor source:**
- `crates/sandbox/agent-guest-proto/src/envelope.rs:42` — `pub version: u32` on `GuestRequest`
- `crates/sandbox/agent-guest-proto/src/envelope.rs:79` — `pub version: u32` on `GuestResponse`
- `crates/sandbox/agent-guest-proto/src/version.rs:23` — `PROTOCOL_VERSION = 1`

**m80 implementation:**
- `crates/m80-proto/src/version.rs` — `pub const PROTOCOL_VERSION: u32 = 1`
- `crates/m80-proto/src/types.rs` — `Envelope<T> { version: u32, kind: String, ... }`
- `crates/m80-proto/src/framing.rs` — `read_frame` version probe at the `VersionProbe` partial-deserialize step

**Test:** `crates/m80-proto/tests/envelope_version.rs::request_and_response_carry_protocol_version`

---

## request-payload

The request envelope carries an opaque payload describing the exec call:
`program: String`, `args: Vec<String>`, optional `cwd: Option<String>`,
optional `env: Option<Vec<(String, String)>>`, optional `stdin: Option<Vec<u8>>`,
`timeout_ms: Option<u64>`, and `streaming: bool`.

No tool name, no agent-tier identity field, no policy field is present on the
request payload.

**Present-tense statement:** `ExecRequest` contains exactly the fields needed
for the guest to spawn a process. It does not carry `tool_call_id`,
`correlation_id`, `idempotency_key`, `effect_class`, `workspace_id`, or
`allowed_tools`. Those are adapter-tier concerns and live in a future
`m80-adapter`, not in m80.

**Rationale for omitted fields (predecessor comparison):**
- `tool_call_id` / `correlation_id` / `idempotency_key` — agent-tier identity
  carriers. m80's `Envelope<T>` carries an opaque `request_id: Option<String>`
  at most; pairing is the caller's job.
- `tool_name` / `arguments` — agent-tier tool catalog. m80 has no tool
  registry; the program and its arguments are passed directly.
- `effect_class` — agent-tier policy. Not m80's concern.
- `workspace_policy` / `allowed_tools` — agent-tier access control. Not m80's concern.
- `artifact_capture_hints` — m80 v0.1 ships inline stdout/stderr only; no
  externalized artifact capture.

**predecessor source:**
- `crates/sandbox/agent-guest-proto/src/envelope.rs:40-68` — `GuestRequest`
  (superset of m80's `ExecRequest`; identity fields dropped for m80)

**m80 implementation:**
- `crates/m80-proto/src/types.rs` — `ExecRequest` struct; the wrapping
  `Envelope<ExecRequest>` carries `kind: "exec_request"` per the
  forward-compat discriminator scheme

**Test:** `crates/m80-proto/tests/envelope_request_payload.rs::serializes_program_args_env_cwd_timeout`

---

## response-payload

The response envelope carries an opaque payload describing the exec outcome:
`status: ExecStatus`, optional `exit_code: Option<i32>`, inline
`stdout: Vec<u8>`, inline `stderr: Vec<u8>`, optional
`truncated: Option<bool>`, `timing: ExecTiming`.

No agent-tier identifier is echoed back in the payload. Response correlation
uses the `request_id` field on the outer `Envelope<T>`.

**Present-tense statement:** `ExecResponse` contains `status`, `exit_code`,
`stdout`, `stderr`, `truncated`, and `timing`. `truncated` is reserved for
the v0.2 externalization story (always `None` in v0.1); the field has
`skip_serializing_if = "Option::is_none"` so v0.1 wire bytes are unchanged.
`ExecResponse` does not carry `tool_call_id`, `error_class` (agent semantics),
`captured_artifacts` (agent-tier externalized output), or any identity field.

**ExecStatus is exhaustive.** Adding a variant requires a `PROTOCOL_VERSION`
bump and a hard cutover of every m80 peer; there is no `#[non_exhaustive]`
escape hatch on this internal pre-1.0 wire enum.

**ExecStatus variants:**
- `Completed` — process exited; `exit_code` is set.
- `TimedOut` — guest killed the process; `exit_code` is typically `None`.
- `Cancelled` — caller requested cancellation.
- `Failed` — exec infrastructure failure before or during execution.

**ExecTiming fields:**
- `spawned_at_unix_ms` — Unix-epoch millisecond timestamp when spawn began.
- `exited_at_unix_ms` — Unix-epoch millisecond timestamp when the process ended.
- `spawn_ms` — wall-clock time from request receipt to spawn.
- `run_ms` — wall-clock time from spawn to exit.

**predecessor source:**
- `crates/sandbox/agent-guest-proto/src/envelope.rs:71-98` — `GuestResponse`
  (superset of m80's `ExecResponse`; identity and artifact fields dropped)

**m80 implementation:**
- `crates/m80-proto/src/types.rs` — `ExecResponse`, `ExecStatus`, `ExecTiming` structs

**Test:** `crates/m80-proto/tests/envelope_response_payload.rs::serializes_status_exit_stdout_stderr_timing`
