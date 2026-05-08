# Cgroup v2 Subtree — Behaviors

## root-path

The system creates each per-VM cgroup leaf under
`/sys/fs/cgroup/m80-firecracker/<vm_id>/`. The parent directory
`/sys/fs/cgroup/m80-firecracker/` is the shared root for all VMs on the host;
it is created if absent. The `vm_id` segment is the caller-supplied VM
identifier, matching the string used as the jailer's `--id` flag.

The name `m80-firecracker` replaces predecessor's `predecessor-firecracker` root to
scope the hierarchy to the m80 process.

Source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/cgroup.rs`
`DEFAULT_FIRECRACKER_CGROUP_ROOT` (line 13) and `unified_v2_leaf_path`
(lines 155–157).

Test: `crates/m80-cgroup/tests/cgroup/subtree.rs::leaf_under_renamed_root`.

## subtree-control

Before enrolling any process, the system recursively writes the needed
controllers to `cgroup.subtree_control` from `/sys/fs/cgroup` through
`/sys/fs/cgroup/m80-firecracker`. The default profile needs `cpu`, `memory`,
`pids`, and `io`; a custom profile without any `io.*` setting only needs
`cpu`, `memory`, and `pids`. If a controller is not listed in an ancestor's
`cgroup.controllers`, the write fails and
`CgroupError::ControllerNotEnabled("<name>")` is returned.

The controller writes and cgroup limit writes happen before PID enrolment.
This keeps controller properties visible and configured before
`cgroup.procs` is mutated.

Source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/cgroup.rs`
`materialize_jailed_cgroup` lines 84–93; `REQUIRED_CONTROLLERS` line 16.

Test: `crates/m80-cgroup/src/lib.rs::tests::required_subtree_control_enables_three_controllers`.
Test: `crates/m80-cgroup/src/lib.rs::tests::create_applies_limits_before_pid_enrollment`.
The latter asserts `+cpu`, `+memory`, `+pids`, and `+io` are present on both
ancestor levels in the synthetic hierarchy before process enrollment.

## sparse-cpuset-inheritance

If the leaf exposes `cpuset.cpus` or `cpuset.mems` and either file is empty,
m80 copies the nearest non-empty ancestor value before PID enrolment. If the
file exists but no ancestor supplies a value, creation fails with
`CgroupError::SparseInheritedFile` rather than enrolling the VM into an
ambiguous cpuset.

Test: `crates/m80-cgroup/src/lib.rs::tests::create_applies_limits_before_pid_enrollment`.

## pid-assign

After the leaf directory is created and limits are applied, the system writes the deduped, sorted
`jailer_pid` and `firecracker_pid` set to `<leaf>/cgroup.procs`. On m80's
non-daemonized jailer launch path those pids are normally equal because the
jailer execs into Firecracker; the dedupe keeps that common case to one write
while preserving the contract if a future launch path reports distinct pids.

Source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/cgroup.rs`
`materialize_jailed_cgroup` lines 101–110.

Test: `crates/m80-cgroup/tests/cgroup/subtree.rs::pids_assigned_to_leaf`.
Test: `crates/m80-cgroup/src/lib.rs::tests::pid_assignment_sorts_and_deduplicates`.

## cgroup-path-txt

After the leaf is created and pids are assigned, the absolute path of the leaf
is written as a single line to `<run_dir>/cgroup-path.txt`. This file is used
for offline triage (e.g., inspecting the cgroup from a separate terminal after
a crash) and for recovery if the orchestrator needs to clean up without an
in-memory `Subtree` handle.

Source: m80 design; not present in predecessor (predecessor passes the path via
return value only).

Test: `crates/m80-cgroup/tests/integration_root.rs::subtree_creation_places_leaf_under_m80_firecracker`
(#[ignore]).

## live-proc-drop

If a `Subtree` is dropped while the leaf still has live processes, the cgroup
leaf remains in place because the kernel rejects `rmdir` on a busy cgroup. The
drop path logs the failed `rmdir` and does not panic; after the live processes
are killed, normal cleanup can remove the leaf.

Test: `crates/m80-cgroup/src/lib.rs::tests::cgroup_drop_with_live_procs_does_not_rmdir`
(#[ignore]).
