# Graceful Stop

## X86 Graceful

m80 v0.1 does not use Firecracker's `SendCtrlAltDel` action on x86_64. Normal
stop is architecture-independent: the host sends `ShutdownRequest` to m80-guestd
over the guest exec vsock channel, waits for the shutdown RPC to return or fail,
and then SIGKILLs the Firecracker process. This deliberately differs from the
older predecessor behavior recorded at
`crates/sandbox/agent-sandbox-firecracker/src/lifecycle.rs:970-981,1286,1297-1305`.

The guestd acknowledgement proves guest userspace has flushed and accepted the
shutdown request. m80 does not wait for a kernel poweroff/reboot event after
that acknowledgement because the observable host cleanup path is the same:
drop the jail/cgroup guards, keep the stopped run directory for optional
extraction, and let `StoppedSandbox::delete` remove it when the caller chooses.

## Non-X86 Forced

m80 v0.1 has no architecture-specific normal-stop split. Non-x86 hosts use the
same guestd shutdown RPC plus Firecracker SIGKILL path as x86 hosts. The
explicit force path is `RunningSandbox::force_kill`, which skips the guest RPC
and SIGKILLs the Firecracker pid plus the jailer pid when that pid is not the
`0` no-live-jailer sentinel.

## Sigkill Escalation

Normal stop always ends by sending SIGKILL to the Firecracker process. If the
guestd shutdown RPC fails because guestd is unreachable or already gone, m80
logs the RPC failure and still sends SIGKILL. The only bounded wait in this path
is the shutdown RPC attempt (`SHUTDOWN_RPC_TIMEOUT`); there is no additional
30-second graceful-poweroff wait in current m80.

The failed-RPC path still returns `StoppedSandbox` after bounded stop and
release have run. The stopped handle keeps ownership of the run directory until
the caller chooses `delete()` or `preserve_for_triage()`. After `delete()`, the
run directory is gone and the admission permit is available for another sandbox.

If the host cannot prove the forced kill completed, cleanup release is blocked.
`RunningSandbox::force_kill` records
`CleanupReleaseBlocker::ForcedKillAmbiguous` in diagnostics, returns the
underlying kill error, and intentionally does not release the admission permit.
That keeps the host from reusing capacity while an unproven Firecracker process
may still own the run directory or jail resources.

## Idempotent Stop

Stop is type-state guarded rather than runtime-idempotent. `RunningSandbox::stop`
and `RunningSandbox::force_kill` consume `RunningSandbox` and return
`StoppedSandbox`, so safe Rust callers cannot call stop twice on the same
running handle. Repeated cleanup of the stopped run directory is represented by
the separate `StoppedSandbox::delete` / `preserve_for_triage` phase and stale
run-root recovery.

## Evidence

- `crates/m80-firecracker/tests/stop_disposition_real_kvm.rs::stop_disposition_normal_records_normal_stop`
- `crates/m80-firecracker/tests/stop_disposition_real_kvm.rs::stop_disposition_force_records_force_kill`
- `crates/m80-firecracker/tests/stop_disposition_real_kvm.rs::forced_kill_ambiguous_blocks_release`
- `crates/m80-firecracker/tests/stop_disposition_real_kvm.rs::kill_pid_best_effort_ignores_no_live_jailer_sentinel`
- `crates/m80-firecracker/tests/stop_disposition_real_kvm.rs::stop_with_unreachable_guestd_still_returns_stopped_and_releases_after_delete`
