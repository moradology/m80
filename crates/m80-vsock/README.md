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
     `ready_marker` (default `GUESTD_READY`) on its own line, or times out.
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
- The guest port is fixed at **9001** by default (the same port
  `m80-guestd` listens on). Callers may override via `Channel::open_with_port`,
  but doing so is unusual and only exists for testing.
- One `Channel` is one connection. Concurrent connections to the same VM
  are not supported in v0.1; opening a second `Channel` against an
  already-open VM returns `VsockError::ChannelInUse`.
- `Channel` is `Drop`-safe: dropping it removes the host-side UDS and
  flushes pending writes. Repeated drops are no-ops.
- Ready-marker timeout maps to `VsockError::NotReady` and is a hard
  failure — the caller's only recovery is to tear down the VM.

## Public surface

- `Channel::open(...)`, `Channel::send(&mut Envelope<T>)`,
  `Channel::recv() -> Envelope<U>`, `Channel::close()`.
- `cid_for_vm_id(vm_id: &str) -> u32`.
- `READY_MARKER_DEFAULT: &str = "GUESTD_READY"`.
- `GUEST_PORT_DEFAULT: u32 = 9001`.
- `VsockError`: `NotReady`, `ConnectFailed { errno }`, `HandshakeFailed`,
  `ChannelInUse`, `Io(io::Error)`, `Proto(m80_proto::ProtoError)`.

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
- `serde`, `thiserror`, `tracing`.
- (No `tokio` — synchronous, like the rest of the host stack.)

## Tests

- Loopback: a fake serial-console file that emits the ready marker at a
  controllable delay; `Channel::open` returns within the timeout.
- Timeout: ready marker never appears; `Channel::open` returns
  `VsockError::NotReady` after the configured timeout.
- Frame round-trip: a fixture envelope is sent, echoed by a stub guest
  process, and received unchanged.
- Drop cleanup: dropping a `Channel` removes the UDS file from disk.
