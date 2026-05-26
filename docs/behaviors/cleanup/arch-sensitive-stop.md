# Stop Disposition

## Current m80 Stop Paths

v0.1 m80 does not use Firecracker's x86-only `SendCtrlAltDel` action. Normal
stop is architecture-independent:

1. open a fresh vsock channel to m80-guestd
2. send `ShutdownRequest`
3. read the bounded shutdown response
4. SIGKILL the Firecracker pid after the RPC returns or fails

`RunningSandbox::force_kill` is the explicit host-only path. It skips the guest
RPC and SIGKILLs the Firecracker pid plus the jailer pid when that pid is not
the `0` no-live-jailer sentinel.

This intentionally differs from the older predecessor arch-sensitive strategy. The
behavior that matters for m80 is not "x86 graceful vs aarch64 unsupported"; it
is "guestd shutdown then host kill" versus "host force kill".

Tests:
- `crates/m80-firecracker/tests/cleanup/arch_sensitive_stop.rs::current_stop_path_is_arch_independent_guestd_shutdown_then_kill`
- `crates/m80-firecracker/tests/cleanup/arch_sensitive_stop.rs::force_kill_is_explicit_host_kill_path`
- `crates/m80-firecracker/tests/cleanup/arch_sensitive_stop.rs::disposition_set_has_no_unsupported_arch_special_case`

## Disposition Record

The public cleanup vocabulary records the two current stop dispositions:
`GuestdShutdownThenFirecrackerKill` and `HostForceKill`. Adding another stop
path is a behavior change and must update the README, behavior docs, and tests.
