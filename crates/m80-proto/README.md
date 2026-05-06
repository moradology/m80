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
- **Cancel envelope (`cancel_request` / `cancel_ack`).** `CancelRequest`
  carries the `request_id` to kill; `CancelAck` replies with one of three
  `CancelStatus` outcomes: `Cancelled`, `AlreadyExited`, or `Failed`.
  Shared with the `m80-5vha` streaming-exec epic — these types are defined
  here once; that epic imports without re-declaring.
  See `docs/behaviors/lifecycle/exec-cancellation.md`.
- **Streaming exec is opt-in.** v0.2 adds
  `ExecRequest::streaming: bool` with `default` +
  `skip_serializing_if = is_false`, so `streaming == false` remains
  byte-identical to the v0.1 request wire. When `streaming == true`, the
  response is zero or more `exec_stdout` / `exec_stderr` envelopes followed
  by exactly one `exec_exit` terminal envelope. All frames carry the original
  `Envelope::request_id`. See `docs/design/wire-streaming-exec.md`.
- **PTY exec is a separate terminal protocol.** Interactive sessions use
  `pty_request` followed by host-to-guest `pty_input`, `pty_resize`, and
  `pty_control` frames, guest-to-host `pty_output` frames, and exactly one
  `pty_exit` terminal frame. PTY output is the merged terminal byte stream;
  it is not modeled as separate stdout and stderr. All frames carry the same
  opaque `Envelope::request_id`. See
  `docs/behaviors/wire-protocol/pty.md`.
- **File operations are direct guest verbs.** `file_read`, `file_write`,
  `file_list`, `file_stat`, `file_remove`, and the chunked write
  `file_write_begin` / `file_write_chunk` / `file_write_commit` sequence
  move bytes without spawning a shell. Responses carry `Option<FileError>`
  with `NotFound`, `PermissionDenied`, `IsADirectory`, `NotADirectory`,
  `SymlinkRejected`, `TooLarge`, or `Io`. `FileRead` defaults to
  `FILE_READ_LIMIT_DEFAULT` (16 MiB) and reports `truncated`; chunked write
  returns an upload id and acks each chunk. See
  `docs/design/wire-fops.md`.

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
- **No agent/TUI opinions.** PTY payloads carry terminal bytes, resize
  events, host-originated control events, and process status only. They do
  not name shells, Claude, editors, tool catalogs, or agent policy.

## Dependencies

`serde`, `serde_json`, `base64`, `thiserror`. None of the other m80 crates.

## Public Surface

File-op exports: `FileReadRequest/Response`, `FileWriteRequest/Response`,
`FileListRequest/Response`, `FileStatRequest/Response`,
`FileRemoveRequest/Response`, `FileWriteBeginRequest/Response`,
`FileWriteChunkRequest/Response`, `FileWriteCommitRequest/Response`,
`FileError`, `FileKind`, `DirEntry`, `FileStat`, `FILE_READ_LIMIT_DEFAULT`,
and their `PAYLOAD_KIND_*` constants.
