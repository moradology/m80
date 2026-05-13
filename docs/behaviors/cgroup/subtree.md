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

## probe-cache

`Subtree::probe()` treats the cgroup mount layout as host-static for the life
of the process. The first call reads `/proc/mounts`, verifies a cgroup v2
mount at `/sys/fs/cgroup`, and reads root `cgroup.subtree_control` to prove the
root interface is visible. The result, including typed failure, is cached
process-wide. Later calls replay the cached result instead of re-reading
`/proc/mounts` or root cgroup files.

Test: `crates/m80-cgroup/src/tests.rs::probe_cache_reuses_successful_result`.
Test: `crates/m80-cgroup/src/tests.rs::probe_cache_replays_first_error`.
Test: `crates/m80-cgroup/src/tests.rs::public_probe_reads_host_once_from_fresh_process`.

## subtree-control

Before enrolling any process, the system recursively writes the needed
controllers to `cgroup.subtree_control` from `/sys/fs/cgroup` through
`/sys/fs/cgroup/m80-firecracker`. The preset profile needs `cpu`, `memory`,
and `pids`; a custom profile with any `io.*` setting also needs `io`.

The root controller list is checked once before the chain write. If a
required controller is not listed in root `cgroup.controllers`, creation fails
with `CgroupError::ControllerNotEnabled("<name>")`. Descendant availability is
not re-read: once the parent write succeeds, the child inherits the controller,
and the kernel remains authoritative for the actual `cgroup.subtree_control`
write.

The controller writes and cgroup limit writes happen before PID enrolment.
This keeps controller properties visible and configured before
`cgroup.procs` is mutated.

Source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/cgroup.rs`
`materialize_jailed_cgroup` lines 84–93; `REQUIRED_CONTROLLERS` line 16.

Test: `crates/m80-cgroup/src/lib.rs::tests::required_subtree_control_enables_three_controllers`.
Test: `crates/m80-cgroup/src/lib.rs::tests::create_applies_limits_before_pid_enrollment`.
Test: `crates/m80-cgroup/src/lib.rs::tests::subtree_control_chain_checks_only_root_controller_availability`.
Test: `crates/m80-cgroup/src/lib.rs::tests::subtree_control_chain_still_rejects_missing_root_controller`.
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

If a `Subtree` is dropped while the leaf still has live processes, Drop writes
`1` to leaf `cgroup.kill` when that kernel file is present, then removes the
leaf with `rmdir`. This asks the kernel to empty the cgroup atomically before
directory removal. If `cgroup.kill` is absent or either write/rmdir operation
fails, Drop logs the failure and does not panic.

Test: `crates/m80-cgroup/src/lib.rs::tests::kill_cgroup_writes_kernel_kill_file_when_present`.
Test: `crates/m80-cgroup/src/lib.rs::tests::drop_without_cgroup_kill_removes_empty_temp_leaf`.
Test: `crates/m80-cgroup/src/lib.rs::tests::cgroup_drop_with_live_procs_uses_cgroup_kill_then_rmdir`
(#[ignore]).
