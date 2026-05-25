# Vsock Connection Lifecycle

## per-request

Each exec request opens a fresh host-side `UnixStream` to the per-VM bridge
socket (`<run_dir>/vsock.sock`), sends `CONNECT <port>\n`, completes the
request/response exchange, then closes. Connections are not pooled or reused.
Concurrent connections to the same VM are not supported in v0.1.

**Implementation:** `Channel::open_uds_only` in `crates/m80-vsock/src/lib.rs`.

**Predecessor source:** `crates/sandbox/agent-sandbox-firecracker/src/vsock.rs:80-95`
(`request_response` calls `connect_reader` + `send_connect` on every call).

## connect-ack

After sending `CONNECT <port>\n` the host reads a single acknowledgement line.
Any line that does not start with `"OK "` is rejected as
`VsockError::HandshakeFailed`. A well-formed ack has the form
`OK <host_port>\n`; the host port value is present but not used further
(Firecracker assigns it dynamically). The ack line is bounded to 64 bytes and
must include the newline terminator. Oversized or unterminated replies fail
closed as `HandshakeFailed` before the host allocates an unbounded buffer.

**Predecessor source:** `crates/sandbox/agent-sandbox-firecracker/src/vsock.rs:130-152`
(`send_connect` ack parsing) and test `rejects_unexpected_connect_acknowledgement`.

## stream-timeouts

Every bridge `UnixStream` is configured with a 5-second read and write timeout
only while `Channel::open_uds_only` performs the `CONNECT` / `OK` handshake.
Before the channel is returned for application traffic, read and write
timeouts are cleared. A valid exec that produces no output for more than five
seconds can still complete; guest-side `ExecRequest::timeout_ms` or an
explicit host deadline owns exec duration.

Callers that need a host-side receive budget use
`Channel::recv_raw_with_deadline`. That wrapper sets bounded per-read timeouts
while checking the caller's absolute deadline, including slow-drip partial
frames, and returns `Ok(None)` when the full frame does not arrive in time.

**Predecessor source:** `crates/sandbox/agent-sandbox-firecracker/src/vsock.rs:13`
(`DEFAULT_VSOCK_BRIDGE_TIMEOUT: Duration = Duration::from_secs(5)`) and
lines 97-117 (`set_read_timeout` / `set_write_timeout`).
