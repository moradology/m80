# `m80-proto`

The wire format that m80's host and guest speak. Pure types + serde; no I/O,
no policy, no transport.

## Reason for being

The host and the in-VM daemon (`m80-guestd`) live in different processes,
different kernels, and — usually — different target triples. They still have
to agree byte-for-byte on the envelope they exchange. `m80-proto` is the
single crate both sides depend on so divergence is impossible.

## Black-box contract

The load-bearing wire invariants — the things consumers cannot derive from
`cargo doc` alone:

- **Hard cutover, no negotiation.** Every envelope carries `version: u32 = 1`
  and `negotiate_version` is exact-match. There is no rolling-upgrade window
  and no host-side translation shim. Bumping `PROTOCOL_VERSION` is an atomic
  redeploy of both peers.
- **NDJSON, one record per line, 4 MiB cap.** The size check is strict `>`:
  exactly `MAX_FRAME_BYTES` passes; `MAX_FRAME_BYTES + 1` is rejected. The
  cap is on encoded JSON, not raw `Vec<u8>` bytes (~33% base64 inflation).
- **Bounded read.** `read_frame` reads at most `MAX_FRAME_BYTES + 2` bytes
  through `Read::take`; an unbounded peer cannot grow the host's heap.
- **Three end-of-read shapes.** Peer closed cleanly before sending →
  `Io(UnexpectedEof)`. Peer closed mid-frame (no `\n`, under cap) →
  `Io(UnexpectedEof)`. Cap hit without `\n` → `OversizedPayload`.
- **`kind` discriminator on `Envelope`** — reserved for v0.2+ payload-type
  extension without bumping `PROTOCOL_VERSION`. Stamped by the constructors
  via the `Payload` trait.
- **`ExecResponse::truncated: Option<bool>`** — reserved for v0.2; always
  `None` in v0.1; `skip_serializing_if` so v0.1 wire bytes are unchanged.
- **Adding an `ExecStatus` variant requires a `PROTOCOL_VERSION` bump.**
  No `#[non_exhaustive]` escape hatch — wire compat is the contract.
- **`request_id` is opaque.** The protocol echoes it back unchanged and
  assigns no meaning; pairing is the consumer's job.

## Non-goals

- **No transport.** `m80-proto` does not own a vsock socket, a UDS handle,
  or anything that `accept()`s. Transports live in `m80-vsock` and
  `m80-guestd`.
- **No semantic identifiers.** The wire carries `request_id`, not
  `tool_call_id` / `correlation_id` / `idempotency_key`. Higher-level
  semantic IDs are an adapter concern.
- **No tool catalog.** The payload is opaque from `m80-proto`'s
  perspective; it does not validate that a request names a known operation.
- **No externalized output.** v0.1 ships inline stdout/stderr only.

## Dependencies

`serde`, `serde_json`, `base64`, `thiserror`. None of the other m80 crates.
