# Lifecycle Error Variants

## api-socket-timeout

`m80-firecracker` raises
`FcError::ApiSocketTimeout { path, timeout }` when the jailed Firecracker
process does not expose its REST API socket within the launch budget. This is a
pre-boot lifecycle failure, not a configuration parse error.

predecessor source: `crates/sandbox/agent-sandbox-firecracker/src/errors.rs:778-781`.

Test: `crates/m80-firecracker/tests/error_variant_displays.rs::api_socket_timeout_is_typed`.

## console-marker-timeout

The current m80 launch path uses an inverted-readiness vsock connection instead
of a serial-console marker. When m80-guestd does not connect within the ready
budget, the orchestrator raises
`FcError::GuestdReadyTimeout { path, timeout }`. The per-VM
`console.log` remains the diagnostic source for guest stderr and boot timing,
but readiness itself is no longer inferred from parsing arbitrary console
lines.

predecessor source: `crates/sandbox/agent-sandbox-firecracker/src/errors.rs:937-942`.

Test: `crates/m80-firecracker/tests/error_variant_displays.rs::guestd_ready_timeout_is_typed`.

## failure-kinds

m80 keeps the lifecycle failure vocabulary bounded by
`LifecycleFailureKind::ALL`: `GuestdNotReady`, `BrokenVsock`, `StuckVm`,
`GracefulStopTimeout`, `ForcedKillFallback`, `CleanupFailure`, and
`WritebackSkippedAfterUncleanStop`. The top-level `FcError` still preserves the
concrete source error so callers do not lose path, timeout, or subsystem detail.

predecessor source:
`crates/sandbox/agent-sandbox-firecracker/src/errors.rs:12-23`.

Test: `crates/m80-firecracker/tests/error_variant_displays.rs::lifecycle_failure_kinds_are_bounded`.
