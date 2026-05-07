# Daemonized Jailer Launch

`SandboxConfig::daemonize` and `JailerConfig::daemonize` map to Firecracker
jailer's `--daemonize` flag. m80 does not implement its own double-fork path;
the official jailer opens `/dev/null`, forks twice, calls `setsid`, redirects
stdio, writes `firecracker.pid`, and execs Firecracker in the daemon process.

m80 waits for `firecracker.pid` and then reaps the short-lived jailer parent.
The persisted `firecracker_pid` is the daemon process. The persisted
`jailer_pid` is `0`, the same no-live-jailer-parent sentinel used by
`new_pid_ns`.

The Firecracker API socket remains the management surface for daemonized VMs.
Host lifecycle code must treat PID 0 as a sentinel, not as a process-group kill
target.

Tests:
- `crates/m80-jailer/src/materialized.rs::tests::launch_with_daemonize_reaps_jailer_parent_and_records_daemon_pid`
- `crates/m80-firecracker/src/lifecycle.rs::tests::kill_pid_zero_is_no_live_jailer_sentinel`
- `crates/m80-jailer/tests/plan_serde.rs::resource_limits_pid_namespace_daemonize_and_netns_persist_in_plan_json`
- `crates/m80-firecracker/tests/end_to_end_real_kvm.rs::end_to_end_real_kvm_daemonized_boot_exec_stop_delete`
