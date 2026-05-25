# Cgroup v2 Failure Modes — Behaviors

## requires-jailer

predecessor surfaces `CgroupRequiresJailer` when `cgroup_mode = unified-v2` but
`jailer_mode != Enabled`. In m80, the typed equivalent is the combination of:
1. The orchestrator checking `BackendConfig::cgroup_mode` before calling
   `Subtree::create`.
2. `Subtree::create` requiring a `&JailedFirecracker` argument — a type that
   can only be obtained from a successful `MaterializedJail::launch()`.

The `CgroupError::RequiresJailer` variant was not declared in `m80-cgroup`
because no code path inside the crate produces it. The type system enforces
the precondition: if a `&JailedFirecracker` can be passed, a jailer is alive.

The orchestrator-level gate (refusing `Subtree::create` when cgroup mode is
not `unified-v2`) lives in `m80-firecracker`.

Source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/cgroup.rs`
`verify_cgroup_host_preflight` (lines 49–72).

Test: `crates/m80-cgroup/tests/cgroup/failure_modes.rs::requires_jailer`.
The runtime probe path is also tested in
`crates/m80-cgroup/tests/probe_no_v2.rs`.

## sparse-inheritance-and-invalid-limits

If a kernel exposes a sparse inherited cgroup file such as `cpuset.cpus` or
`cpuset.mems` on the leaf and no ancestor has a non-empty value, creation
fails with `CgroupError::SparseInheritedFile`. If a requested limit is outside
the kernel-accepted range, such as `io.weight = 0` or an `oom_score_adj`
outside `-1000..=1000`, m80 returns `CgroupError::InvalidLimit`.

Test: `crates/m80-cgroup/src/lib.rs::tests::create_applies_limits_before_pid_enrollment`.
Test: `crates/m80-cgroup/src/lib.rs::tests::io_weight_range_is_kernel_bounded`.

## cleanup-idempotent

Drop on `Subtree` writes `1` to leaf `cgroup.kill` when that file is present,
then removes the per-VM leaf cgroup with `fs::remove_dir`. Both steps are
best-effort: if the kill write fails, or if `rmdir` still fails (e.g.,
`ENOENT` because the directory was already removed), a `tracing::warn!` is
emitted and the error is swallowed. No panic.

`cleanup_orphan_subtree(vm_id)` implements the same idempotent pattern for
recovery at startup:
- Non-existent path → `Ok(())`.
- `cgroup.procs` non-empty → return `CgroupError::LivePids`; orchestrators must
  preserve the matching run-dir so a future same-`vm_id` launch cannot adopt
  the live pids.
- `cgroup.procs` empty → `rmdir` the leaf.

`m80-firecracker::Backend::recover_stale_run_root()` maps the live-pid case to
`FcError::StaleCgroupLeaf` and does not delete the run-dir. See
`docs/behaviors/cgroup/stale-live-pid-recovery.md`.

m80 does not attempt to clean up the parent `m80-firecracker/` directory
automatically; the parent persists across VM lifetimes and is shared.

Source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/cgroup.rs`
`cleanup_materialized_cgroup` (lines 127–149).

Test: `crates/m80-cgroup/tests/cgroup/failure_modes.rs::cleanup_idempotent`.
