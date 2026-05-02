# Wire Protocol — Frame Transport

Behaviors captured by bead epic `m80-g3x`, leaves `m80-g3x.2.1` through
`m80-g3x.2.3`.

---

## ndjson

The transport serializes each envelope as a single NDJSON record: compact JSON
(no pretty-printing) terminated by a single `\n`. There are no continuation
frames and no embedded raw newlines inside a record.

**Present-tense statement:** `write_frame` serializes the envelope to compact
JSON via `serde_json::to_vec`, checks the size, and then appends `b'\n'` before
writing to the `Write` implementor. `read_frame` wraps the reader in a bounded
`Read::take(MAX_FRAME_BYTES + 2)` and calls `BufRead::read_until(b'\n', ...)`
to consume exactly one `\n`-terminated line within the cap, strips the trailing
newline (and a leading `\r`, for CRLF tolerance), and then parses.

**predecessor source:**
- `crates/sandbox/agent-guest-proto/src/envelope.rs:292-296` — `ndjson_serialize`:
  `serde_json::to_vec(val)` followed by `buf.push(b'\n')`
- `crates/sandbox/agent-guest-proto/src/envelope.rs:299-310` — `ndjson_deserialize`:
  `bytes.strip_suffix(b"\n")` then size check then `serde_json::from_slice`

**m80 implementation:**
- `crates/m80-proto/src/framing.rs` — `write_frame` and `read_frame`

**Test:** `crates/m80-proto/tests/framing_ndjson.rs::serializes_one_record_per_line`

---

## size-cap

The deserializer rejects any frame whose post-trim payload exceeds 4 MiB
(`MAX_FRAME_BYTES = 4 * 1024 * 1024`) with `ProtoError::OversizedPayload`.

The comparison is **strict `>`**: a frame whose trimmed length equals exactly
`MAX_FRAME_BYTES` bytes passes the size gate. A frame of `MAX_FRAME_BYTES + 1`
bytes is rejected immediately with `OversizedPayload { size: MAX_FRAME_BYTES + 1, limit: MAX_FRAME_BYTES }`.

**Present-tense statement:** `read_frame` reads at most `MAX_FRAME_BYTES + 2`
bytes through `Read::take` (one extra byte beyond the cap so a strict-`>`
check has signal to fire). If `read_until(b'\n', ...)` exhausts the cap
without finding a newline, the frame is rejected as `OversizedPayload` —
peer-driven unbounded-allocation attacks cannot grow the host buffer past the
cap. After the line is read, the trailing `\n` (and an optional `\r`) is
stripped and `trimmed.len()` is checked against `MAX_FRAME_BYTES` with strict
`>`. The encoder (`write_frame`) applies the same guard before writing.

A peer that closes mid-frame (`n_read > 0` and no `\n` and `n_read < cap`)
surfaces as `Io(UnexpectedEof)`, **not** `OversizedPayload` — the cap-hit
variant is reserved for genuine oversize.

**predecessor source:**
- `crates/sandbox/agent-guest-proto/src/envelope.rs:22` — `MAX_NDJSON_PAYLOAD_BYTES = 4 * 1024 * 1024`
- `crates/sandbox/agent-guest-proto/src/envelope.rs:301-306` — `if trimmed.len() > MAX_NDJSON_PAYLOAD_BYTES { return Err(ProtoError::OversizedPayload { ... }) }`

**m80 implementation:**
- `crates/m80-proto/src/version.rs` — `pub const MAX_FRAME_BYTES: usize = 4 * 1024 * 1024`
- `crates/m80-proto/src/framing.rs` — bounded read via `Read::take`, oversize/EOF triage, strict-`>` size gate, write-side cap

**Test:** `crates/m80-proto/tests/framing_size_cap.rs::rejects_frame_above_4mib_with_oversized_error`

---

## parse-failure

When `serde_json` fails to parse a frame, `read_frame` returns
`ProtoError::MalformedPayload(detail)` where `detail` is the
`serde_json::Error::to_string()` output. Connection handlers **must** drop the
connection on this error; the byte stream is in an unrecoverable state and
attempting to read further frames from the same connection is undefined
behaviour at the protocol level.

**Present-tense statement:** A `MalformedPayload` return from `read_frame`
means the framing invariant has been violated. The caller is responsible for
closing the connection. m80's connection handlers (in `m80-guestd` and
`m80-firecracker`) must exit the request loop and close the connection on any
`Err` from `read_frame`, matching predecessor's drop pattern.

**predecessor source:**
- `crates/sandbox/agent-guest-proto/src/envelope.rs:307-310` — `serde_json::from_slice(trimmed).map_err(|e| ProtoError::MalformedPayload { detail: e.to_string() })`
- `services/guestd-rs/src/main.rs:322-323` — `serve_connection` returns on any `Err`; the loop at `serve_vsock_connections:318-326` logs and continues to the next connection (not the same connection)

**m80 implementation:**
- `crates/m80-proto/src/framing.rs` — `read_frame` returns `Err(ProtoError::MalformedPayload(...))` on JSON parse failure
- `crates/m80-proto/src/error.rs` — `ProtoError::MalformedPayload(String)`

**Test:** `crates/m80-proto/tests/framing_parse_failure.rs::malformed_json_returns_error_and_drops_connection`
