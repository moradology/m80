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

Test: The precondition is type-checked at compile time. No runtime test is
needed for the `requires-jailer` path itself; the probe path is tested in
`crates/m80-cgroup/tests/probe_no_v2.rs`.

## cleanup-idempotent

The system removes the per-VM leaf cgroup with `fs::remove_dir`. Drop on
`Subtree` calls this in best-effort mode: if `rmdir` fails (e.g., `EBUSY`
because processes remain, or `ENOENT` because the directory was already
removed), a `tracing::warn!` is emitted and the error is swallowed. No panic.

`cleanup_orphan_subtree(vm_id)` implements the same idempotent pattern for
recovery at startup:
- Non-existent path → `Ok(())`.
- `cgroup.procs` non-empty → warn and return `Ok(())` (live pids; orchestrator
  must kill them first).
- `cgroup.procs` empty → `rmdir` the leaf.

m80 does not attempt to clean up the parent `m80-firecracker/` directory
automatically; the parent persists across VM lifetimes and is shared.

Source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/cgroup.rs`
`cleanup_materialized_cgroup` (lines 127–149).

Test: `crates/m80-cgroup/tests/integration_root.rs::orphan_cleanup_removes_empty_stale_subtree`
(#[ignore] — requires root + real cgroup v2 host).
