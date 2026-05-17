# `m80-proto`

The wire format that m80's host and guest speak. Pure types plus a checked-in
protobuf schema, generated wire structs, and framing helpers; no policy and no
transport ownership.

## Reason for being

The host and the in-VM daemon (`m80-guestd`) live in different processes,
different kernels, and — usually — different target triples. They still have
to agree byte-for-byte on the envelope they exchange. `m80-proto` is the
single crate both sides depend on so divergence is impossible.

## Black-box contract

The load-bearing wire invariants — the things consumers cannot derive from
`cargo doc` alone:

- **Hard cutover, no negotiation.** Every envelope carries the exact
  `PROTOCOL_VERSION` and `negotiate_version` is exact-match. There is no
  rolling-upgrade window and no host-side translation shim. Bumping
  `PROTOCOL_VERSION` is an atomic redeploy of both peers.
- **Checked-in schema.** `proto/m80/wire.proto` owns the protobuf field tags.
  `build.rs` compiles it with `prost-build` and a vendored `protoc`, then the
  crate exposes a narrow typed facade over the generated wire structs.
- **Length-prefixed protobuf, 4 MiB cap.** Every frame is a four-byte
  big-endian body length followed by one protobuf envelope body. The size
  check is strict `>`: exactly `MAX_FRAME_BYTES` passes;
  `MAX_FRAME_BYTES + 1` is rejected. The cap is on the encoded protobuf body.
- **Bounded read.** `read_frame` reads one fixed-size prefix, rejects an
  oversized announced body before allocation, and then reads exactly that
  body length. An unbounded peer cannot grow the host's heap.
- **Three end-of-read shapes.** Peer closed cleanly before sending →
  `Io(UnexpectedEof)`. Peer closed mid-frame after a valid prefix →
  `Io(UnexpectedEof)`. Announced body length over the cap →
  `OversizedPayload`.
- **`kind` discriminator on `Envelope`** — a redundant dispatch and diagnostics
  label stamped by constructors via the `Payload` trait. The protobuf `oneof`
  is the typed payload. A `kind`/payload mismatch fails closed; adding or
  removing payload variants still requires an atomic `PROTOCOL_VERSION` bump.
- **Unknown envelope fields fail closed.** `read_raw_frame` rejects top-level
  envelope field numbers outside the current schema before converting to a
  typed payload. This turns unknown protobuf `oneof` payload tags into an
  explicit `MalformedPayload` with the offending field number instead of
  silently dropping them as unknown protobuf data.
- **`ExecResponse::truncated: Option<bool>`** — whether stdout/stderr was
  truncated in buffered responses reconstructed from stream chunks.
- **Adding an `ExecStatus` variant requires a `PROTOCOL_VERSION` bump.**
  No `#[non_exhaustive]` escape hatch — wire compat is the contract.
- **`request_id` is opaque.** The protocol echoes it back unchanged and
  assigns no meaning; pairing is the consumer's job.
- **`max_duration_ms` is an optional envelope deadline.** It bounds the whole
  request/response call in wall-clock milliseconds. Guestd currently applies it
  to exec and PTY requests by taking the minimum of the payload timeout and the
  envelope deadline.
- **Cancel envelope (`cancel_request` / `cancel_response`).** `CancelRequest`
  carries the `request_id` to kill; `CancelResponse` replies with one of three
  `CancelStatus` outcomes: `Cancelled`, `AlreadyExited`, or `Failed`.
  Shared with the `m80-5vha` streaming-exec epic — these types are defined
  here once; that epic imports without re-declaring.
  See `docs/behaviors/lifecycle/exec-cancellation.md`.
- **Streaming exec is opt-in (shipped v0.1).** `ExecRequest::streaming: bool`
  selects the real-time response shape. When `streaming == true`, the
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
  `file_list`, `file_stat`, `file_remove`, `file_mkdir`, and the chunked write
  `file_write_begin` / `file_write_chunk` / `file_write_commit` sequence
  move bytes without spawning a shell. Responses carry `Option<FileError>`
  with `NotFound`, `PermissionDenied`, `IsADirectory`, `NotADirectory`,
  `SymlinkRejected`, `TooLarge`, `InvalidSequence`, or `Io`. `FileRead` defaults to
  `FILE_READ_LIMIT_DEFAULT` (16 MiB) and returns one or more
  `file_read_chunk` frames ending in `done: true`; the terminal chunk reports
  `truncated` or `error`. Direct writes are bounded by the active encoded
  protobuf frame cap before guestd dispatch sees them. Chunked write returns an
  upload id and acks each chunk with the matching upload id and sequence.
  Byte-heavy read/write chunk frames use protobuf `bytes` fields on
  the active wire, not base64 strings. See `docs/design/wire-fops.md`.
- **Guest metrics are fixed-shape.** `metrics_request` has an empty payload;
  `metrics_response` carries typed CPU tick counters from `/proc/stat`, memory
  gauges from `/proc/meminfo`, and guestd request/error counters. There is no
  free-form key/value map. See
  `docs/behaviors/observability/guest-metrics-vsock.md`.
- **Guest health probes are direct verbs.** `ping_request` has an empty
  payload; `pong_response` carries `guest_unix_ms`, the guestd wall-clock
  timestamp in Unix milliseconds at handling time. The verb does not spawn a
  guest process.
- **Pmem mount is a direct guest verb.** `pmem_mount_request` asks guestd to
  mount already-attached `/dev/pmem<N>` devices at admitted guest layer paths.
  `pmem_mount_response` returns one status per requested device, with bounded
  `PmemMountError` values for device absence, mount failure, missing DAX,
  invalid device path, invalid mount path, or I/O failure.
- **Post-restore hooks are closed and host-sequenced.**
  `post_restore_hook_request` carries a 32-byte host-generated restore nonce
  plus ordered hook variants: reseed systemd random-seed, regenerate
  machine-id, and set a validated hostname. Responses carry one typed result
  per hook with bounded `HookError` values. There is no custom command, argv
  passthrough, `Other(String)`, or generic hook payload. An empty hook list is
  still a meaningful restore signal: guestd mixes the nonce and reseeds the
  kernel CRNG before returning an empty success result list.
- **No implicit event stream.** The protocol does not carry unsolicited OOM
  events or DNS proxy frames on the request/response channel. Those require
  explicit future payloads and a `PROTOCOL_VERSION` bump.

## Public surface

Frame I/O: `read_frame`, `write_frame`, `read_raw_frame`, `write_raw_frame`.
`write_raw_frame` emits each length-prefixed frame with one `write_all` over
the coalesced prefix+body buffer.
Raw protobuf helpers: `RawEnvelope`, `RawEnvelope::from_typed(&Envelope<T>)`,
`encode_raw_envelope`.
Frame sizing and ports: `MAX_FRAME_BYTES`, `PROTOCOL_VERSION`,
`GUEST_PORT_DEFAULT`, `READY_PORT_DEFAULT`, `READY_MARKER_DEFAULT`.

Envelope and traits: `Envelope<T>`, `Payload` (implemented by all payload
types).

Core exec types: `ExecRequest`, `ExecResponse`, `ExecStdout`, `ExecStderr`,
`ExecExit`, `ExecStatus`, `ExecTiming`, `CancelRequest`, `CancelResponse`,
`CancelStatus`, and `PAYLOAD_KIND_EXEC_REQUEST`,
`PAYLOAD_KIND_EXEC_RESPONSE`, `PAYLOAD_KIND_EXEC_STDOUT`,
`PAYLOAD_KIND_EXEC_STDERR`, `PAYLOAD_KIND_EXEC_EXIT`,
`PAYLOAD_KIND_CANCEL_REQUEST`, `PAYLOAD_KIND_CANCEL_RESPONSE`.

Shutdown types: `ShutdownRequest`, `ShutdownResponse`, `ShutdownAction`,
`PAYLOAD_KIND_SHUTDOWN_REQUEST`, and `PAYLOAD_KIND_SHUTDOWN_RESPONSE`.

PTY types: `PtyRequest`, `PtyOutput`, `PtyInput`, `PtyResize`, `PtyControl`,
`PtyControlEvent`, `PtySignal`, `PtySize`, `PtyExit`, and
`PAYLOAD_KIND_PTY_REQUEST`, `PAYLOAD_KIND_PTY_INPUT`,
`PAYLOAD_KIND_PTY_OUTPUT`, `PAYLOAD_KIND_PTY_RESIZE`,
`PAYLOAD_KIND_PTY_CONTROL`, `PAYLOAD_KIND_PTY_EXIT`.

Drive hotplug types: `DriveMountRequest`, `DriveMountResponse`,
`DriveMountSpec`, `DriveMountStatus`, `DriveMountStatusKind`,
`DriveDetachRequest`, `DriveDetachResponse`, `DriveDetachSpec`,
`DriveDetachStatus`, `DriveDetachStatusKind`, `DriveHotplugError`,
`TenantIdentityReport`, and their `PAYLOAD_KIND_*` constants. These carry
VM-mechanics data only: Firecracker drive ids, guest mount paths, per-device
status/error values, explicit guest block-device paths, and opaque tenant
identity bytes.

Pmem mount types: `PmemMountRequest`, `PmemMountResponse`, `PmemMountSpec`,
`PmemMountStatus`, `PmemMountStatusKind`, `PmemMountError`, and
`PAYLOAD_KIND_PMEM_MOUNT_REQUEST` / `PAYLOAD_KIND_PMEM_MOUNT_RESPONSE`. These
carry VM-mechanics data only: guest pmem device path, guest mount path,
per-device status/error values, and the opaque erofs image digest.

Post-restore hook types: `PostRestoreHookRequest`,
`PostRestoreHookResponse`, `HookKindWire`, `HookHostname`, `HookResultWire`,
`HookStatus`, `HookError`, and `PAYLOAD_KIND_POST_RESTORE_HOOK_REQUEST` /
`PAYLOAD_KIND_POST_RESTORE_HOOK_RESPONSE`. These carry VM-lifecycle data only:
the restore nonce and the closed hook/result taxonomy.

File-op exports: `FileReadRequest`, `FileReadChunk`,
`FileReadResponse`, `FileWriteRequest`, `FileWriteResponse`,
`FileListRequest`, `FileListResponse`, `FileStatRequest`, `FileStatResponse`,
`FileRemoveRequest`, `FileRemoveResponse`, `FileMkdirRequest`,
`FileMkdirResponse`, `FileWriteBeginRequest`,
`FileWriteBeginResponse`, `FileWriteChunkRequest`, `FileWriteChunkResponse`,
`FileWriteCommitRequest`, `FileWriteCommitResponse`,
`FileError`, `FileKind`, `DirEntry`, `FileStat`, `FILE_READ_LIMIT_DEFAULT`,
and their `PAYLOAD_KIND_*` constants.

Guest metrics exports: `MetricsRequest`, `MetricsResponse`,
`GuestCpuMetrics`, `GuestMemMetrics`, and
`PAYLOAD_KIND_METRICS_REQUEST` / `PAYLOAD_KIND_METRICS_RESPONSE`.

Guest health exports: `PingRequest`, `PongResponse`, and
`PAYLOAD_KIND_PING_REQUEST` / `PAYLOAD_KIND_PONG_RESPONSE`.

Error type: `ProtoError`.

Generated wire module: `wire::generated` is generated from
`proto/m80/wire.proto` and marked `#[doc(hidden)]`. Normal callers use the typed
payload structs and frame helpers; generated structs are only for protocol
plumbing and variant work inside `m80-proto`. The one public generated variant
type, `wire::WirePayload`, exists for malicious-protocol harnesses that need to
construct mismatched or intentionally malformed envelopes; production callers
should use typed payloads through `Envelope<T>`.

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

`prost`, `prost-derive`, `thiserror`. Build-only dependencies are
`prost-build`, `protoc-bin-vendored`, `sha2`, `hex`, and `indexmap` pinned for
the workspace Rust toolchain. The build script prefers a caller-supplied
`PROTOC`; when it falls back to the vendored Linux x86_64 binary, it verifies
that binary's sha256 before executing it. None of the other m80 crates.

## Tests

- `tests/fileops_round_trip.rs` — each file-op request/response pair
  round-trips through `write_frame` + `read_frame` byte-equivalent.
- `tests/hotplug_round_trip.rs` — drive mount/detach payloads round-trip,
  preserve partial-success statuses, and carry tenant identity as opaque bytes.
- `tests/pmem_round_trip.rs` — pmem mount request/response payloads
  round-trip and preserve typed failure statuses.
- `tests/post_restore_hooks_round_trip.rs` — post-restore hook
  request/response payloads round-trip, preserve exact 32-byte restore nonces,
  validate hostnames on decode, and reject wrong-length nonces.
- `tests/shutdown_round_trip.rs` — shutdown request/response payloads
  round-trip for every declared shutdown action.
- Unit tests in-crate: `Envelope` kind/payload mismatch fails closed,
  `OversizedPayload` fires at `MAX_FRAME_BYTES + 1`, EOF before prefix vs.
  EOF mid-frame both map to `Io(UnexpectedEof)`, `negotiate_version`
  rejects any version other than `PROTOCOL_VERSION`.
- `tests/envelope_unknown_fields.rs` — unknown top-level envelope fields fail
  closed with the offending field number.
