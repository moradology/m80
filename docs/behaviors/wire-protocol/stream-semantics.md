# Wire Protocol - Stream Semantics

Behaviors captured by WRA0.5.

## State machine

Every streaming request has one opaque `request_id` on the outer envelope. m80
does not allocate semantic agent stream ids. The active stream identity is the
tuple `(request_id, kind)` for exec stdout/stderr and PTY output/control, or the
upload id returned by `file_write_begin_response` for chunked uploads.

Each stream follows:

```text
Open -> Data* -> Done|Error|Canceled
```

Illegal transitions observed before the terminal frame fail closed:

- A missing terminal frame before peer disconnect is
  `DisconnectBeforeTerminal` with a `DisconnectCause` naming the host-visible
  cause after a short Firecracker PID-state confirmation.
- A response envelope whose `request_id` is missing or belongs to another
  request is `RequestIdMismatch`.
- Sequence gaps or repeats are `SequenceMismatch`.

The host returns as soon as it receives the terminal frame for a request and
then closes that request channel. Frames a peer writes after the terminal frame
are outside the active request and are not drained to classify a second error.

## Sequence rules

Sequences are monotonic within one stream and start at zero.

- `exec_stdout` and `exec_stderr` have independent `u32` sequences. They may
  interleave on the same connection; wire order is the observed cross-stream
  order.
- `file_read_chunk` uses one `u64` sequence for the read response. The terminal
  chunk has `done=true`; it may carry zero bytes.
- `file_write_chunk_request` uses one `u64` sequence within the upload id
  returned by `file_write_begin_response`. Guestd rejects gaps and duplicates
  with `FileError::InvalidSequence`, removes the failed upload, and unlinks the
  temp file; the host validates each ack echoes the same upload id and expected
  sequence before sending the next chunk.
- `pty_input` and `pty_output` have independent `u32` sequences. `pty_resize`
  and `pty_control` share the host control sequence.

Request/response envelopes that are not streams have exactly one response
frame. Collision is impossible inside m80 because one connection carries one
request or one explicit upload session; a caller that reuses an upload id after
commit or failure receives a typed file error.

## Terminal rules

Exec streaming ends with exactly one `exec_exit`. PTY streaming ends with
exactly one `pty_exit`, except same-connection cancellation may end with
`cancel_ack` instead of a PTY exit. File reads end with exactly one
`file_read_chunk { done=true }`; errors are represented on that terminal chunk.

Explicit cancellation sends `cancel_request` on the same connection. A
successful cancel returns `cancel_ack { status=cancelled }` and no later exec or
PTY terminal frame. A timeout is a terminal exec/PTY outcome, not a cancel ack.
A host disconnect without `cancel_request` is treated by guestd as cancellation
of the in-flight child and produces no host-visible terminal frame because the
host has already gone away.

A malformed host control frame during streaming is equivalent to a poisoned
current channel: guestd logs a `protocol_error` with the active `request_id`
and `stream_id=control`, kills the in-flight child, and drops the current
connection without emitting a terminal `exec_exit` or `pty_exit`. A fresh
channel to the same guest remains usable.

## Bounded memory

The global protobuf frame cap remains 4 MiB, so a peer cannot force one frame
allocation above that limit. Streaming producers emit bounded chunks instead of
collecting whole stdout, stderr, PTY output, or file contents in memory. The
host buffered `exec` wrapper is intentionally separate: it reconstructs an
`ExecResponse` from streaming chunks and caps each collected stdout/stderr
buffer at 1 MiB. Direct streaming exec and PTY callers are not capped at that
buffered wrapper limit; their terminal `truncated` fields remain `false` unless
a future explicit streaming/PTY cap is added.

Chunk sizes are implementation choices below the frame cap. Guest file reads
write each chunk directly to the response stream. Exec and PTY producer threads
feed synchronous channels with a one-frame bound, so a slow host writer can
hold at most one waiting output frame per channel before producer reads block.
m80 does not insert an unbounded in-process queue between the guest producer
and host consumer.

## Evidence

- `crates/m80-proto/tests/framing_parse_failure.rs` rejects malformed protobuf
  and unsupported versions on the active protobuf wire.
- `crates/m80-firecracker/src/lifecycle/exec.rs::tests::response_frame_rejects_stale_request_id`,
  `response_frame_rejects_missing_request_id`, and
  `cancel_ack_rejects_stale_request_id` pin host-side stale response rejection.
- `crates/m80-firecracker/src/lifecycle/exec.rs::tests::stream_sequence_gap_returns_protocol_error`
  and `stream_sequence_duplicate_returns_protocol_error` pin host-side exec and
  PTY sequence rejection.
- `crates/m80-firecracker/src/lifecycle/fileops.rs::tests::chunk_ack_rejects_wrong_sequence`
  and `chunk_ack_rejects_wrong_upload_id` pin malformed upload acks.
- `crates/m80-guestd/src/connection/fileops/tests.rs::chunked_upload_rejects_sequence_gap_without_writing`
  and `chunked_upload_rejects_duplicate_sequence_without_mutating_total` pin
  guest-side malformed upload requests.
- `crates/m80-guestd/src/connection/protocol_log.rs` tests pin protocol
  diagnostics for malformed, oversized, version-mismatch, and unexpected-frame
  cases with request and stream context.
- `crates/m80-firecracker/tests/wire_frame_boundaries_real_kvm.rs::malformed_frame_mid_stream_maps_to_user_visible_error`
  pins real-KVM mid-stream malformed control-frame handling: current-channel
  teardown, guest console `protocol_error` visibility, and fresh-channel
  survival.
- `crates/m80-guestd/tests/streaming_exec.rs` pins stdout/stderr per-stream
  sequence monotonicity, terminal exit, cancel ack, and disconnect cleanup.
- `crates/m80-guestd/tests/pty_exec.rs` pins PTY output, cancel ack, and
  disconnect cleanup.
- `crates/m80-guestd/tests/fileops.rs` pins multi-chunk file reads and chunked
  upload commit behavior.
- `crates/m80-guestd/src/connection/streaming.rs` pins the exec output channel
  bound with `stream_frame_channel_is_bounded_to_one_waiting_frame`.
- `crates/m80-proto/tests/pty_round_trip.rs` pins interleaved PTY control/input
  protobuf frames preserving order and request ids.
- `crates/m80-firecracker/src/lifecycle/exec.rs` and
  `crates/m80-firecracker/src/lifecycle/fileops.rs` map malformed, oversized,
  unsupported-version, unexpected-frame, sequence-mismatch, and
  disconnect-before-terminal cases into `WireProtocolError`.
