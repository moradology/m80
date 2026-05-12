# `m80-vsock`

Host-side connection management for the vsock channel between m80 and the
in-VM daemon. Speaks `m80-proto` envelopes over Firecracker's UDS-to-vsock
bridge.

## Reason for being

Vsock on the host is not a plain socket. Firecracker exposes a Unix domain
socket that proxies to the guest's vsock CID/port; using that bridge correctly
means writing the connect line, validating the OK response, and then streaming
framed protocol messages.

This crate owns that bridge handshake and frame transport. Readiness is a
separate orchestrator concern: `m80-firecracker` pre-creates the inverted-ready
listener, accepts `m80-guestd`'s ready connection, and only then asks
`m80-vsock` to open the exec channel.

## Black-box contract

- `Channel::open_uds_only(host_uds: &Path, guest_port: u32) -> Result<Channel,
  VsockError>` connects to the supplied Firecracker UDS, sets five-second
  read/write timeouts, sends `CONNECT <guest_port>`, validates an `OK ...`
  response, and returns a duplex channel that reads/writes
  `m80-proto::Envelope` frames.
- The caller supplies the full host UDS path. `m80-vsock` does not construct
  run-root paths or assume layout above that socket.
- Readiness is already established before `Channel::open_uds_only` is called.
  The inverted-ready listener and timeout live in `m80-firecracker`, not here.
- The CID is derived deterministically from the VM id; consumers do not manage
  a CID pool. `cid_for_vm_id(&str) -> u32` maps into
  `3..=u32::MAX - 1`, avoiding `0`, `VMADDR_CID_HYPERVISOR`,
  `VMADDR_CID_HOST`, and `VMADDR_CID_ANY`.
- The guest exec port is fixed at `9001` by default
  (`m80_proto::GUEST_PORT_DEFAULT`). Callers import the port from `m80-proto`,
  not through `m80-vsock`.
- One `Channel` is one connection. The Firecracker UDS is the VM's listener;
  callers may open a fresh sequential `Channel` for each request. Concurrent
  connections to the same VM are not supported in v0.1.
- `Channel::try_clone_sender()` returns a write-only clone for same-connection
  control frames. It is not a second request lane; it exists so the host can
  send `cancel_request` while the owning channel is blocked waiting for exec
  output.
- `Channel` is `Drop`-safe: dropping it flushes pending writes and closes the
  connection. It does not remove the host-side UDS; the orchestrator owns
  socket cleanup during VM teardown/restore.

## Timeouts

`BRIDGE_IO_TIMEOUT` is five seconds. It is applied as both the read timeout and
the write timeout on every Firecracker UDS stream opened by
`Channel::open_uds_only`.

## Public surface

- `Channel::open_uds_only(...)`, `Channel::send(&mut Envelope<T>)`,
  `Channel::recv() -> Envelope<U>`, `Channel::recv_raw() -> RawEnvelope`,
  and `Channel::try_clone_sender()`.
- `ChannelSender::send(&mut Envelope<T>)` for same-connection control frames.
- `cid_for_vm_id(vm_id: &str) -> u32`.
- `VsockError`: `HandshakeFailed`, `Io { path: PathBuf, source: io::Error }`,
  `Proto(m80_proto::ProtoError)`.

## Non-goals

- **No tool dispatch.** `Channel` carries opaque envelopes; it does not inspect
  their contents.
- **No multiplexing.** One channel is one in-flight request. Pipelining is out
  of scope.
- **No readiness ownership.** The inverted-ready listener is part of
  `m80-firecracker` launch sequencing. `m80-vsock` has no serial-console
  marker watcher.
- **No socket cleanup.** The Firecracker UDS is owned by the VM run root and is
  cleaned up by the orchestrator.

## Dependencies

- `m80-proto` for the envelope and framing helpers.
- `sha2`, `thiserror`, `tracing`.
- No `tokio`: this crate is synchronous like the rest of the host foundation.

## Debug instrumentation

Set `M80_DEBUG_WIRE=vsock` or `M80_DEBUG_WIRE=all` to enable wire-level logging
via `tracing::trace!`. When enabled, every vsock handshake line and every frame
sent or received is logged with a hex+ASCII preview of up to 1024 bytes.

- Matching is exact (`==`): `M80_DEBUG_WIRE= vsock` with a leading space does
  not match; `M80_DEBUG_WIRE=vsock` does.
- Multiple targets are comma-separated: `M80_DEBUG_WIRE=vsock,fcrest`.
- `all` matches every target.
- Unknown tokens are silently ignored.
- The gate is a single atomic load on the hot path; no serialization occurs
  unless the gate fires.

See `crates/m80-firecracker/README.md` "Debug instrumentation" for the
complete table of all recognized `M80_DEBUG_WIRE` targets across the workspace.

## Tests

- `tests/cid_for_vm_id.rs` checks determinism, reserved-CID avoidance, and
  pinned SHA-256 CID values for stability across hash-function changes.
- `tests/handshake.rs` checks successful handshake, bad ack to
  `HandshakeFailed`, and missing UDS to `Io`.
- `tests/frame_round_trip.rs` checks envelope round trips and cloned-sender
  same-connection control frames.
- `tests/drop_cleanup.rs` checks that dropping a `Channel` leaves the
  Firecracker UDS listener in place for future connections.
- `crates/m80-firecracker/src/launch.rs` owns the inverted-readiness tests:
  listener path, protocol byte validation, timeout, and exec-channel probe.
