# Wire Protocol - Version Check

Behaviors captured by bead epic `m80-g3x`, leaves `m80-g3x.3.1` and
`m80-g3x.3.2`.

---

## application frames

Every active application payload is carried in a length-prefixed protobuf
`Envelope` with `version: u32`, `kind`, optional opaque `request_id`, and a
typed `oneof` payload. The reader validates `version == PROTOCOL_VERSION`
before dispatching the payload. There is no NDJSON prelude, no JSON/base64
compatibility path, and no separate application-channel protocol byte.

**Present-tense statement:** When a host/guest application frame is read,
`read_raw_frame` decodes the protobuf body and rejects any envelope whose
`version` differs from `PROTOCOL_VERSION`. The connection handler logs and
surfaces that protocol error instead of trying an older wire shape.

The only live protocol byte outside an application frame is the inverted
boot-readiness signal: after binding its exec listener, `m80-guestd` connects
to the host ready port and writes one `PROTOCOL_VERSION` byte. The host treats
that byte as "guestd is listening and speaks this build's protocol." It is not
a reusable application-channel handshake.

**m80 implementation:**
- `crates/m80-proto/src/framing.rs` — `read_raw_frame` checks each envelope version.
- `crates/m80-proto/src/wire.rs` — protobuf `WireEnvelope` decode/encode.
- `crates/m80-guestd/src/main.rs` — ready signal writes one `PROTOCOL_VERSION` byte.
- `crates/m80-firecracker/src/launch.rs` — ready signal validates that byte before
  opening the exec channel.

**Tests:**
- `crates/m80-proto/tests/envelope_version.rs` pins envelope version stamping.
- `crates/m80-proto/tests/framing_parse_failure.rs` pins version-mismatch rejection.
- `crates/m80-proto/tests/handshake_exchange.rs` keeps the reserved
  `HandshakeMessage` payload as an exact-match protobuf round trip; it is not
  an active connection prelude.

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

**Test:** `crates/m80-proto/src/version.rs::tests::rejects_newer_version`
