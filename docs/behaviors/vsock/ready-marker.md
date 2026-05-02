# Vsock Ready-Marker Probe

## console-watch

After `InstanceStart`, the host scans the per-VM serial console log file for
the ready-marker token on its own line. The first appearance of the token
signals that the guest daemon is bound and accepting vsock connections.

**Implementation:** `m80_vsock::watch_ready_marker` in
`crates/m80-vsock/src/lib.rs`. Called by `Channel::open` before any connect
attempt. Polls the console file every 50 ms.

**Predecessor source:** `crates/sandbox/agent-sandbox-firecracker/src/lifecycle.rs:697`
(`wait_for_console_marker`) and `vsock.rs:14`
(`DEFAULT_GUESTD_READY_MARKER = "GUESTD_READY"`).

## manifest-token

The ready-marker token is caller-supplied to `Channel::open` as `ready_marker:
&str`. The conventional default (`READY_MARKER_DEFAULT = "GUESTD_READY"`) is
exported so the orchestrator layer can read it from the rootfs manifest and
pass it down, overriding the default if the manifest specifies a different
token.

**Predecessor source:** `crates/sandbox/agent-sandbox-firecracker/src/backend.rs:122`
(`ready_marker: DEFAULT_GUESTD_READY_MARKER.to_owned()`).

## timeout

The host waits for the ready marker for at most the `timeout` argument passed
to `Channel::open`. When the deadline is exceeded `VsockError::NotReady` is
returned; the caller's only recovery is to tear down the VM.

The predecessor default was 45 seconds (`DEFAULT_READY_TIMEOUT`). m80 does not
hard-code this default; it is the caller's responsibility to pass an
appropriate timeout from the image manifest or configuration.

**Predecessor source:** `crates/sandbox/agent-sandbox-firecracker/src/backend.rs:27`
(`DEFAULT_READY_TIMEOUT: Duration = Duration::from_secs(45)`) and
`lifecycle.rs:697`.

## ordering

The ready probe runs as the first step of `Channel::open`, before the
`UnixStream::connect` call. This ensures the guest daemon is bound before the
host attempts the vsock bridge handshake, so callers never see a connection
failure on the first request due to a race between VM boot and the connect
attempt.

**Predecessor source:** `crates/sandbox/agent-sandbox-firecracker/src/lifecycle.rs:531-722`
(preboot pipeline: ready wait occurs between `InstanceStart` and the first
vsock dial).
