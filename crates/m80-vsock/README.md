# `m80-vsock`

Host-side connection management for the vsock channel between m80 and the
in-VM daemon. Speaks `m80-proto` envelopes over Firecracker's UDS-to-vsock
bridge.

## Reason for being

Vsock on the host is not a plain socket. Firecracker exposes a UDS that
proxies to the guest's vsock CID/port; using that UDS correctly involves
a small dance (write the connect line, read the OK, then stream). The
ready-marker probe via the serial console is a separate, brittle dance
of its own.

Both belong in one crate so that nothing else in the codebase has to
think about either. The orchestrator calls `connect()`, gets an
`m80-proto`-shaped channel, and goes about its day.

A second motivation: keeping the vsock surface small lets us swap the
transport later (e.g., when Firecracker grows native vsock-over-virtio in
some future) without touching the orchestrator.

## Black-box contract

- `Channel::open(host_uds: &Path, guest_port: u32, ready_marker: &str,
  console: &Path, timeout: Duration) -> Result<Channel, VsockError>`
  performs:
  1. Watches the serial console file at `console` until it sees
     `ready_marker` (default `m80_proto::READY_MARKER_DEFAULT`) on its own line, or times out.
  2. Connects to the host UDS, performs the Firecracker connect-line
     handshake to `guest_port`, and verifies the OK response.
  3. Returns a duplex channel that reads/writes `m80-proto::Envelope`
     frames.
- The CID is **derived deterministically from the VM id**; consumers do not
  need to manage a CID pool. The crate exposes `cid_for_vm_id(&str) -> u32`
  for callers that want to inspect.
- The host UDS is fixed at `<run_dir>/vsock.sock`. The crate constructs
  the path from a `run_dir` argument; it does not assume any layout above
  that.
- The guest port is fixed at **9001** by default (`m80_proto::GUEST_PORT_DEFAULT`).
  Callers pass the port directly to `Channel::open`; no separate
  `open_with_port` variant is needed.
- One `Channel` is one connection. The Firecracker UDS is the VM's listener;
  callers may open a fresh sequential `Channel` for each request. Concurrent
  connections to the same VM are not supported in v0.1.
- `Channel::try_clone_sender()` returns a write-only clone for same-connection
  control frames. It is not a second request lane; it exists so the host can
  send `cancel_request` while the owning channel is blocked waiting for exec
  output.
- `Channel` is `Drop`-safe: dropping it flushes pending writes and closes the
  connection. It does **not** remove the host-side UDS; the orchestrator owns
  socket cleanup during VM teardown/restore.
- Ready-marker timeout maps to `VsockError::NotReady` and is a hard
  failure — the caller's only recovery is to tear down the VM.

## Public surface

- `Channel::open_uds_only(...)`, `Channel::send(&mut Envelope<T>)`,
  `Channel::recv() -> Envelope<U>`, `Channel::try_clone_sender()`, and
  `Channel::close()`.
- `ChannelSender::send(&mut Envelope<T>)` and `ChannelSender::close()` for
  same-connection control frames.
- `cid_for_vm_id(vm_id: &str) -> u32`.
- `READY_MARKER_DEFAULT` and `GUEST_PORT_DEFAULT` — re-exported from
  `m80-proto`, where the canonical values live.
- `watch_ready_marker(console: &Path, marker: &str, timeout: Duration) -> Result<(), VsockError>` —
  the ready-probe extracted as a standalone helper (also useful in tests).
- `VsockError`: `NotReady`, `ConnectFailed { errno }`, `HandshakeFailed`,
  `Io(io::Error)`, `Proto(m80_proto::ProtoError)`.
  (`ChannelInUse` removed — the caller is responsible for not double-opening;
  no global registry is maintained.)

## Non-goals

- **No tool dispatch.** `Channel` carries opaque envelopes; it doesn't
  inspect their contents.
- **No multiplexing.** One channel = one in-flight request. Pipelining is
  out of scope.
- **No serial-console parsing beyond the ready marker.** The console file
  is a sentinel-watch target only; m80 doesn't try to capture guest logs
  here.

## Dependencies

- `m80-proto` — for the envelope and framing helpers.
- `serde`, `sha2`, `thiserror`, `tracing`.
- (No `tokio` — synchronous, like the rest of the host stack.)

## Debug instrumentation

Set `M80_DEBUG_WIRE=vsock` (or `M80_DEBUG_WIRE=all`) to enable wire-level
logging via `tracing::trace!`. When enabled, every vsock handshake line and
every frame sent or received is logged with a hex+ASCII preview of up to
1024 bytes.

- Matching is exact (`==`): `M80_DEBUG_WIRE= vsock` (leading space) does
  **not** match; `M80_DEBUG_WIRE=vsock` does.
- Multiple targets are comma-separated: `M80_DEBUG_WIRE=vsock,fcrest`.
- `all` matches every target.
- Unknown tokens are silently ignored.
- The gate is a single atomic load on the hot path; no serialization occurs
  unless the gate fires.

## Tests

- `tests/cid_for_vm_id.rs` — determinism, reserved-range avoidance, and
  pinned SHA-256 CID values for stability across hash-function changes.
- `tests/ready_marker_watch.rs` — marker present immediately, marker appears
  after delay, timeout case, partial-line non-match, missing console file.
- `tests/handshake.rs` — successful handshake, bad ack → `HandshakeFailed`,
  missing UDS → `ConnectFailed`.
- `tests/frame_round_trip.rs` — send `Envelope<ExecRequest>`, receive
  `Envelope<ExecResponse>` via a stub server.
- `tests/frame_round_trip.rs` — cloned sender emits a same-connection
  `cancel_request` while the primary channel remains open for reads.
- `tests/drop_cleanup.rs` — dropping a `Channel` and calling `close()`
  both leave the Firecracker UDS listener in place for future connections.
