# Vsock Ready Signal

## console-watch

m80 v0.1 does not watch the serial console for readiness. Readiness is an
inverted vsock signal owned by `m80-firecracker`: before `InstanceStart`, the
host pre-creates a `UnixListener` at `<vsock.sock>_<READY_PORT_DEFAULT>` using
Firecracker's muxer port-suffix convention. After `m80-guestd` binds its exec
listener, it connects out to that ready port and writes the readiness protocol
byte.

`m80-vsock` only opens exec channels after the orchestrator has observed this
signal. There is no `watch_ready_marker` helper and no serial-console polling
in this crate.

**Implementation:** `phase_11b_ready_listener` and
`phase_12b_ready_accept` in `crates/m80-firecracker/src/launch.rs`;
`m80_vsock::Channel::open_uds_only` in `crates/m80-vsock/src/lib.rs`.

**Predecessor source:** `crates/sandbox/agent-sandbox-firecracker/src/lifecycle.rs:697`
(`wait_for_console_marker`) and `vsock.rs:14`
(`DEFAULT_GUESTD_READY_MARKER = "GUESTD_READY"`). m80 intentionally replaces
that serial-marker probe with the inverted-ready listener.

## manifest-token

`READY_MARKER_DEFAULT = "GUESTD_READY"` remains in `m80-proto` as a legacy
guest log/manifest identity value, but it is not load-bearing for m80 launch
readiness. The readiness contract is the fixed `READY_PORT_DEFAULT` and the
single protocol-version byte written by `m80-guestd` after its exec listener is
bound.

**Implementation:** `READY_PORT_DEFAULT` and `READY_MARKER_DEFAULT` in
`crates/m80-proto/src/lib.rs`; ready-accept validation in
`crates/m80-firecracker/src/launch.rs`.

## timeout

The host waits for the inverted-ready connection for `READY_TIMEOUT` (60
seconds). On deadline expiry, launch fails closed with
`FcError::GuestdReadyTimeout`; the caller's recovery is to tear down the VM
run root and retry with a fresh launch.

**Implementation:** `READY_TIMEOUT` and `phase_12b_ready_accept` in
`crates/m80-firecracker/src/launch.rs`.

## ordering

The ready listener is created before `InstanceStart`, so the guest can connect
as soon as `m80-guestd` has bound its exec port. After `InstanceStart`, the
host accepts the ready connection, validates the protocol byte, and then opens
the exec channel with `Channel::open_uds_only`. The sandbox is not reported as
running until the ready signal and exec-channel probe have both succeeded.

**Implementation:** launch phases 11b, 12a, and 12b in
`crates/m80-firecracker/src/launch.rs`.
