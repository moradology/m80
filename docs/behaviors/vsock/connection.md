# Vsock Connection Lifecycle

## per-request

Each exec request opens a fresh host-side `UnixStream` to the per-VM bridge
socket (`<run_dir>/vsock.sock`), sends `CONNECT <port>\n`, completes the
request/response exchange, then closes. Connections are not pooled or reused.
Concurrent connections to the same VM are not supported in v0.1.

**Implementation:** `Channel::open` in `crates/m80-vsock/src/lib.rs`.

**Predecessor source:** `crates/sandbox/agent-sandbox-firecracker/src/vsock.rs:80-95`
(`request_response` calls `connect_reader` + `send_connect` on every call).

## connect-ack

After sending `CONNECT <port>\n` the host reads a single acknowledgement line.
Any line that does not start with `"OK "` is rejected as
`VsockError::HandshakeFailed`. A well-formed ack has the form
`OK <host_port>\n`; the host port value is present but not used further
(Firecracker assigns it dynamically).

**Predecessor source:** `crates/sandbox/agent-sandbox-firecracker/src/vsock.rs:130-152`
(`send_connect` ack parsing) and test `rejects_unexpected_connect_acknowledgement`.

## stream-timeouts

Every bridge `UnixStream` is configured with a 5-second read and write timeout
(`BRIDGE_IO_TIMEOUT`) immediately after connecting. A wedged guest cannot stall
the host indefinitely on any single I/O call; a timed-out operation surfaces as
`VsockError::Io` with `ErrorKind::WouldBlock` or `TimedOut`.

**Predecessor source:** `crates/sandbox/agent-sandbox-firecracker/src/vsock.rs:13`
(`DEFAULT_VSOCK_BRIDGE_TIMEOUT: Duration = Duration::from_secs(5)`) and
lines 97-117 (`set_read_timeout` / `set_write_timeout`).
