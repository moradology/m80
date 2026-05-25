# Stale Live Cgroup PID Recovery

Behavior capture for bead `m80-ul5ji.1`.

## Contract

Run-root recovery must not delete the host run directory when the matching
cgroup leaf still contains live pids. Deleting the run directory while
preserving the cgroup leaf removes the ownership evidence that prevents a later
same-`vm_id` launch from reusing the occupied leaf.

When recovery sees `CgroupError::LivePids`, it:

- preserves the run directory;
- preserves `network-state.json` and does not run network cleanup;
- returns `FcError::StaleCgroupLeaf { vm_id, path, pids }`.

Ordinary network cleanup errors remain best-effort and do not block stale
run-dir deletion. A live cgroup leaf is not ordinary cleanup residue because it
can create cross-VM containment drift on `vm_id` reuse.

`m80-cgroup::Subtree::create` also checks an existing leaf's `cgroup.procs`
before applying limits or enrolling the new jailer/Firecracker pids. A non-empty
leaf fails closed with `CgroupError::LivePids`.

## Verification

- `crates/m80-firecracker/src/backend.rs::tests::remove_run_dir_preserves_state_when_cgroup_has_live_pids`
- `crates/m80-cgroup/src/tests.rs::create_rejects_existing_leaf_with_live_pids_before_enrollment`
