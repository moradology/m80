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

Before creating the per-VM leaf, the system writes `+cpu +memory +pids` to
`/sys/fs/cgroup/m80-firecracker/cgroup.subtree_control`. This enables the
three controllers on the parent so the leaf inherits them. If a controller is
not listed in the parent's `cgroup.controllers`, the write fails and
`CgroupError::ControllerNotEnabled("<name>")` is returned.

The write must happen before `mkdir` of the leaf because cgroup v2 does not
propagate controllers retroactively; the kernel rejects the mkdir if the parent
hasn't enabled them first.

Source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/cgroup.rs`
`materialize_jailed_cgroup` lines 84–93; `REQUIRED_CONTROLLERS` line 16.

Test: `crates/m80-cgroup/src/lib.rs::tests::required_subtree_control_enables_three_controllers`.

## pid-assign

After the leaf directory is created, the system writes the deduped, sorted
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
