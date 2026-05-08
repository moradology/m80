# Start And Ready Detection

## Console Marker

m80 v0.1 does not tail the serial console for `GUESTD_READY` before declaring a
VM ready. Instead, the host pre-binds an inverted ready listener at
`<vsock.sock>_<READY_PORT_DEFAULT>` before `InstanceStart`. Once guestd has
bound its exec listener, guestd connects out to that ready port and writes the
m80 protocol-version byte. The host accepts that event and treats it as the
boot-completion signal.

This is intentionally different from the older predecessor behavior recorded at
`crates/sandbox/agent-sandbox-firecracker/src/lifecycle.rs:697`, which tailed
the console log for a marker string. The serial console remains diagnostic
output in `<run_dir>/console.log`, not the readiness synchronization channel.

## Ready Timeout

The ready accept loop is bounded by `READY_TIMEOUT` (60 seconds in production).
If no guestd ready connection arrives before the deadline, m80 returns
`FcError::GuestdReadyTimeout { path, timeout }` carrying the ready-listener path
and timeout. This is the m80 equivalent of predecessor's fail-closed
`ConsoleMarkerTimeout` path, but the watched object is the host ready listener
rather than the console marker.

## Vsock Probe

After the ready signal is accepted and the protocol byte is validated,
`Sandbox::launch` returns `RunningSandbox` without opening a dummy exec-channel
connection. The ready signal is emitted only after guestd has bound the exec
listener; consuming that same sequential listener during launch creates an
avoidable handoff race before the caller's first real exec under fully saturated
hosts. The first actual operation opens the normal guest exec channel through
the request path's bounded retry loop.

Receive-side guest protocol readiness is therefore two-step:

1. guestd connects out to the ready listener and writes the protocol byte;
2. the caller's first operation opens the normal guest exec channel.

## Guest Port

The guest exec channel uses the fixed m80 guestd port `9001`
(`m80_vsock::GUEST_PORT_DEFAULT`). The ready signal uses
`m80_proto::READY_PORT_DEFAULT`; it is separate from the exec channel and only
exists to make boot readiness event-driven.

## Cold Boot Only

`Backend::admit(config).launch()` is a cold-boot path. It creates a new per-VM
run directory, materializes a fresh jail, applies preboot REST PUTs, and starts
Firecracker for that sandbox. Warm reuse is explicit through `WarmPool`; the
backend admission path does not silently lease or reuse a warm slot.
