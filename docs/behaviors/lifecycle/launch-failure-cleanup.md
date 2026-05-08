# Launch Failure Cleanup

## Behavior

If a cold launch reaches a live Firecracker process but fails before returning
`RunningSandbox`, m80 owns the partial VM state and cleans it up before the
error reaches the caller. A Firecracker process that never creates its REST API
socket returns `FcError::ApiSocketTimeout`. A stalled guestd readiness signal
returns `FcError::GuestdReadyTimeout`. Both paths force-kill the pre-running
Firecracker/jailer process, drop the jail/cgroup/storage locals, remove the
partial run directory, and release the admission permit.

The same rule applies to snapshot restore. If the snapshot is loaded but the
restored guestd exec channel never becomes ready, restore returns
`FcError::GuestdReadyTimeout` and removes the partial restore run directory
before the caller can admit the next sandbox.

Cleanup is best-effort only where Rust drop cannot return an error: process
kill failures and run-dir removal failures are logged. The public error remains
the launch failure that made the sandbox unavailable.

## Evidence

- `crates/m80-firecracker/tests/lifecycle_failure_real_kvm.rs::api_socket_timeout_cleans_partial_state`
- `crates/m80-firecracker/tests/lifecycle_failure_real_kvm.rs::guestd_not_ready_timeout_cleans_partial_state`
- `crates/m80-firecracker/tests/lifecycle_failure_real_kvm.rs::restore_guestd_not_ready_timeout_cleans_partial_state`

These tests are ignored by default because they require a privileged host and
real m80 artifacts. The API-socket case swaps in a fake Firecracker executable
that never creates the REST socket. The cold-boot guestd case overrides kernel
boot args so guestd never dials the inverted-readiness socket. The restore case
captures a snapshot after stopping guestd, then verifies the restore probe
times out. Each case asserts the typed error, asserts the failed run directory
was removed, and verifies admission can proceed through the same one-permit
backend.
