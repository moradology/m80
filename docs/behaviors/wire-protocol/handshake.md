# Wire Protocol — Version Handshake

Behaviors captured by bead epic `m80-g3x`, leaves `m80-g3x.3.1` and
`m80-g3x.3.2`.

---

## exchange

On every fresh connection, both peers exchange a `HandshakeMessage { version: u32 }`
before processing any application payload. The handshake runs first so an
incompatible peer is detected and the connection closed before any exec work
begins.

**Present-tense statement:** When a connection is established, the initiating
peer writes a `HandshakeMessage` serialized as an NDJSON line. The responder
reads it, calls `negotiate_version(remote.version)`, and writes its own
`HandshakeMessage` in return. Only after both sides have confirmed the version
do they enter the application request loop (`Envelope<ExecRequest>` / `Envelope<ExecResponse>`).

`HandshakeMessage::current()` constructs a handshake stamped to
`PROTOCOL_VERSION`. `negotiate_version(remote)` returns `Ok(())` iff
`remote == PROTOCOL_VERSION`, otherwise `Err(ProtoError::IncompatibleVersion)`.
The error always reports `expected: PROTOCOL_VERSION` regardless of caller
context.

**predecessor source:**
- `services/guestd-rs/src/main.rs:428-439` — `perform_stdio_handshake`: reads one
  handshake line, calls `deserialize_handshake_request`, calls
  `negotiate_handshake`, writes `serialize_handshake_response`, flushes, then
  returns to `serve_stdio_with_workspace_setup` which enters the request loop
- `services/guestd-rs/src/main.rs:402-406` — `perform_stdio_handshake` is called
  before `serve_stdio_request_loop`

**m80 implementation:**
- `crates/m80-proto/src/types.rs` — `HandshakeMessage`, `HandshakeMessage::current()`
- `crates/m80-proto/src/version.rs` — `negotiate_version`

**Test:** `crates/m80-proto/tests/handshake_exchange.rs::handshake_runs_before_first_request`

---

## mismatch

Any version value other than the exact live `PROTOCOL_VERSION` — whether older,
newer, or zero — produces `ProtoError::IncompatibleVersion { expected: PROTOCOL_VERSION, got: remote }`.
There is no range-based fallback, no dual-version dispatch path, and no host-side
shim. Hard-cutover is the only supported upgrade strategy.

**Present-tense statement:** `negotiate_version(remote)` rejects any `remote`
that is not equal to `PROTOCOL_VERSION`, regardless of whether it is less than
or greater than `PROTOCOL_VERSION`. The error always sets `expected` to
`PROTOCOL_VERSION` (the value this binary expects) and `got` to the remote
peer's value, so log messages are self-explanatory without further context.

**Hard-cutover doctrine:** There is exactly ONE live protocol version at any
time. Backward-compatible range checks (`MIN..=MAX` with `MIN < MAX`),
dual-version dispatch paths, and host-side shims that translate old envelopes
are **explicitly forbidden**. If the deploy cannot be atomic, fix the deploy
pipeline — do not weaken the protocol boundary.

**predecessor source:**
- `crates/sandbox/agent-guest-proto/src/version.rs:35-37` — `is_compatible(version: u32) -> bool { version == PROTOCOL_VERSION }` — exact-match only
- `crates/sandbox/agent-guest-proto/src/version.rs:1-23` — hard-cutover doctrine comment (verbatim in m80 `PROTOCOL_VERSION` rustdoc)
- `crates/sandbox/agent-guest-proto/src/envelope.rs:229-234` — `IncompatibleVersion { got, expected }` mapping

**m80 implementation:**
- `crates/m80-proto/src/version.rs` — `negotiate_version`
- `crates/m80-proto/src/error.rs` — `ProtoError::IncompatibleVersion { expected: u32, got: u32 }`

**Test:** `crates/m80-proto/tests/handshake_mismatch.rs::rejects_incompatible_version`
