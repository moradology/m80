# Concurrency — Stop And Same-Id Launch

`ownership.lock` is a run-directory lifetime lease, not a launch-phase marker.
`Sandbox::launch` writes the lock before storage prep and moves the guard into
`RunningSandbox`; `stop()` and `force_kill()` carry it into `StoppedSandbox`.
The guard is released only when the stopped sandbox is deleted, preserved, or
dropped.

This prevents a second `Sandbox::launch` with the same `vm_id` from reusing the
same `<run_root>/<vm_id>` while the first VM is running or while stop teardown
is still carrying live state. The second launch may acquire an admission permit,
but phase 2 must fail on the existing live ownership record before storage prep,
jailer materialization, or Firecracker API work can mutate the run directory.

Test:
`crates/m80-firecracker/tests/concurrency/stop_launch.rs::concurrent_stop_launch_run_dir_race`
is an ignored real-KVM regression that launches VM-A, races VM-A stop against a
VM-B launch with the same `vm_id`, and asserts VM-B fails while VM-A's run-dir
lease is still held.
