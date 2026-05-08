# Launch Failure Cleanup

## Behavior

If a cold launch reaches a live Firecracker process but fails before returning
`RunningSandbox`, m80 owns the partial VM state and cleans it up before the
error reaches the caller. A stalled guestd readiness signal returns
`FcError::GuestdReadyTimeout`, force-kills the pre-running Firecracker/jailer
process, drops the jail/cgroup/storage locals, removes the partial run
directory, and releases the admission permit.

The same rule applies to snapshot restore. If the snapshot is loaded but the
restored guestd exec channel never becomes ready, restore returns
`FcError::GuestdReadyTimeout` and removes the partial restore run directory
before the caller can admit the next sandbox.

Cleanup is best-effort only where Rust drop cannot return an error: process
kill failures and run-dir removal failures are logged. The public error remains
the launch failure that made the sandbox unavailable.

## Evidence

- `crates/m80-firecracker/tests/lifecycle_failure_real_kvm.rs::guestd_not_ready_timeout_cleans_partial_state`
- `crates/m80-firecracker/tests/lifecycle_failure_real_kvm.rs::restore_guestd_not_ready_timeout_cleans_partial_state`

Both tests are ignored by default because they require a KVM-capable host and
real Firecracker artifacts. The cold-boot case overrides kernel boot args so
guestd never dials the inverted-readiness socket. The restore case captures a
snapshot after stopping guestd, then verifies the restore probe times out. Each
case asserts the typed error, asserts the failed run directory was removed, and
then admits and launches a healthy VM through the same one-permit backend.
