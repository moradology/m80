# Wire Protocol — Frame Transport

Behaviors captured by bead epic `m80-g3x`, leaves `m80-g3x.2.1` through
`m80-g3x.2.3`.

---

## length-prefixed protobuf

The transport serializes each envelope as one length-prefixed protobuf frame:
a four-byte big-endian body length followed by that many protobuf body bytes.
There are no newline delimiters and no continuation frames.

**Present-tense statement:** `write_frame` converts the typed envelope into a
raw protobuf envelope, encodes it, checks the encoded body length, writes a
four-byte big-endian length prefix, and then writes the body. `read_frame`
reads exactly the prefix, rejects an announced body larger than
`MAX_FRAME_BYTES`, reads exactly that many body bytes, decodes protobuf, and
then checks `PROTOCOL_VERSION`.

**predecessor source:**
- `crates/sandbox/agent-guest-proto/src/envelope.rs:292-296` — `ndjson_serialize`:
  `serde_json::to_vec(val)` followed by `buf.push(b'\n')`
- `crates/sandbox/agent-guest-proto/src/envelope.rs:299-310` — `ndjson_deserialize`:
  `bytes.strip_suffix(b"\n")` then size check then `serde_json::from_slice`

**m80 implementation:**
- `crates/m80-proto/src/framing.rs` — `write_frame` and `read_frame`

**Test:** `crates/m80-proto/tests/framing_protobuf.rs::serializes_one_length_prefixed_frame_per_envelope`

---

## size-cap

The deserializer rejects any protobuf body whose announced length exceeds 4 MiB
(`MAX_FRAME_BYTES = 4 * 1024 * 1024`) with `ProtoError::OversizedPayload`.

The comparison is **strict `>`**: a frame whose body length equals exactly
`MAX_FRAME_BYTES` bytes passes the size gate. A frame of `MAX_FRAME_BYTES + 1`
bytes is rejected immediately with `OversizedPayload { size: MAX_FRAME_BYTES + 1, limit: MAX_FRAME_BYTES }`.

**Present-tense statement:** `read_frame` allocates the body buffer only after
the four-byte prefix has passed the strict cap check. The encoder
(`write_frame`) applies the same body-length guard before writing. A receiver
that sees `OversizedPayload` closes that channel without writing a typed
payload response, because the peer may already have sent bytes from the rejected
body and the stream is no longer aligned to the next frame boundary.

A peer that closes before the four-byte prefix or before the announced body is
fully read surfaces as `Io(UnexpectedEof)`, **not** `OversizedPayload` — the
cap variant is reserved for a body length that is genuinely too large.

**predecessor source:**
- `crates/sandbox/agent-guest-proto/src/envelope.rs:22` — `MAX_NDJSON_PAYLOAD_BYTES = 4 * 1024 * 1024`
- `crates/sandbox/agent-guest-proto/src/envelope.rs:301-306` — `if trimmed.len() > MAX_NDJSON_PAYLOAD_BYTES { return Err(ProtoError::OversizedPayload { ... }) }`

**m80 implementation:**
- `crates/m80-proto/src/version.rs` — `pub const MAX_FRAME_BYTES: usize = 4 * 1024 * 1024`
- `crates/m80-proto/src/framing.rs` — length prefix read, oversize/EOF triage, strict-`>` size gate, write-side cap

**Test:** `crates/m80-proto/tests/framing_size_cap.rs::rejects_frame_above_4mib_with_oversized_error`

---

## parse-failure

When protobuf decoding fails to parse a frame, `read_frame` returns
`ProtoError::MalformedPayload(detail)` where `detail` is the
decoder error string. Connection handlers **must** drop the
connection on framing and parse errors; the byte stream is in an unrecoverable
state and attempting to read further frames from the same connection is
undefined behaviour at the protocol level.

**Present-tense statement:** A `MalformedPayload` return from `read_frame`
means the framing invariant has been violated. The caller is responsible for
closing the connection. m80's connection handlers (in `m80-guestd` and
`m80-firecracker`) must exit the request loop and close the connection on
poisoned framing errors. A decodable `IncompatibleVersion` frame may still get
a typed failure response because the envelope was parsed and the stream
boundary is known.

**predecessor source:**
- `crates/sandbox/agent-guest-proto/src/envelope.rs:307-310` — `serde_json::from_slice(trimmed).map_err(|e| ProtoError::MalformedPayload { detail: e.to_string() })`
- `services/guestd-rs/src/main.rs:322-323` — `serve_connection` returns on any `Err`; the loop at `serve_vsock_connections:318-326` logs and continues to the next connection (not the same connection)

**m80 implementation:**
- `crates/m80-proto/src/framing.rs` — `read_frame` returns `Err(ProtoError::MalformedPayload(...))` on protobuf parse failure
- `crates/m80-proto/src/error.rs` — `ProtoError::MalformedPayload(String)`

**Tests:**
- `crates/m80-proto/tests/framing_parse_failure.rs::malformed_protobuf_returns_error_and_drops_connection`
- `crates/m80-guestd/tests/handle_connection.rs::oversized_initial_frame_drops_connection_without_response`
