# `m80-cgroup`

Per-VM cgroup v2 subtree under `/sys/fs/cgroup/m80-firecracker/<vm-id>`.
Assigns the jailer + firecracker pids, sets controller limits, cleans up
on VM delete.

## Reason for being

cgroup v2 enforcement is small in code (~300 LOC) but easy to get wrong:
the wrong subtree path leaks state across VMs; failing to write
`cgroup.subtree_control` first makes the controller files invisible;
forgetting to assign both PIDs (jailer's *and* firecracker's) lets the
firecracker child escape the limits when jailer exec's it.

Pulling cgroup logic into its own crate gives that small but tricky
surface a single test boundary, and lets the rest of the codebase opt out
cleanly: hosts without unified cgroup v2 mode skip this crate entirely
without ifdef chains scattered through `m80-firecracker`.

## Black-box contract

- `Subtree::create(vm_id: &str, jail: &MaterializedJail, jailed: &JailedFirecracker) -> Result<Subtree, CgroupError>`
  creates `<root>/m80-firecracker/<vm-id>/`, enables the cpu/memory/pids
  controllers, and assigns both `jailed.jailer_pid` and `jailed.firecracker_pid`
  to the subtree. The constructor requires a live `JailedFirecracker` (the
  result of `MaterializedJail::launch`) because cgroup v2 requires live pids
  to assign. `RequiresJailer` is not returned — the type system enforces the
  precondition.
- `Subtree::apply_limits(&self, &Limits) -> Result<(), CgroupError>`
  writes the per-controller files. `Limits` is a struct with optional
  `cpu_max`, `memory_max`, `pids_max`; setting a field to `None` leaves
  the existing value alone.
- `Subtree::path()` exposes the materialized cgroup path so consumers
  (observability, recovery) can inspect it. The path is also persisted
  in `<run_dir>/cgroup-path.txt` for offline triage.
- Cleanup: `Subtree::Drop` removes the subtree directory if empty;
  errors are logged but don't panic. The orchestrator may call
  `cleanup_orphan_subtree(vm_id)` at startup to handle subtrees from
  prior crashed runs.
- The crate is **mode-gated**: `Subtree::create` is only callable when
  the host has cgroup v2 unified hierarchy mounted at `/sys/fs/cgroup`.
  Hybrid or v1 hosts get `CgroupError::UnsupportedHostMode` from
  `Subtree::probe()`, which is the precondition the orchestrator runs
  at preflight.

## Public surface

- `Subtree::probe() -> Result<(), CgroupError>` — host capability check.
- `Subtree::create(vm_id, &MaterializedJail, &JailedFirecracker) -> Result<Subtree, CgroupError>`.
  Signature takes both the jail (for `plan.config.run_dir`) and the live jailed
  process pair (for the pids to assign). The original single-argument form was
  adjusted because `MaterializedJail` does not carry pids — those live on
  `JailedFirecracker` after `launch()`.
- `Subtree::apply_limits(&Limits)`, `Subtree::path()`.
- `cleanup_orphan_subtree(vm_id: &str)`.
- `Limits { cpu_max: Option<CpuMax>, memory_max: Option<u64>, pids_max: Option<u32> }`.
- `CgroupError`: `UnsupportedHostMode`, `Io { path, source }`,
  `ControllerNotEnabled(String)`.
  **`RequiresJailer` was dropped**: the type signature enforces a live
  `&JailedFirecracker`, so no code path inside this crate produces the variant.
  Per the "no error variants nothing produces" rule, it is not declared.
  The structured `Io { path, source }` form replaces the flat `Io(io::Error)`
  form from the initial type-pinning pass, matching the pattern from
  `m80-image-manifest` and `m80-jailer`.

## Non-goals

- **No cgroup v1 support.** v0.1 is unified-v2 only. Hybrid hosts skip.
- **No automatic limit selection.** `Limits` is an input, not a computed
  default. The orchestrator decides what to enforce.
- **No metrics scraping.** Reading `cpu.stat` / `memory.stat` belongs in
  `m80-observability`.

## Dependencies

- `m80-jailer` — `Subtree::create` requires a `MaterializedJail`.
- `serde` — for the persisted cgroup-path record.
- `thiserror`, `tracing`.

## Tests

- Probe: on a unified-v2 host (CI), `probe()` returns `Ok`. With a fake
  procfs root pointing at a v1 hierarchy, `probe()` returns
  `UnsupportedHostMode`.
- Subtree creation: against a real `MaterializedJail`, the subtree path
  exists, `cpu.max` / `memory.max` / `pids.max` are writable, and both
  pids are listed in `cgroup.procs`.
- Limit enforcement: setting `memory_max = 64MiB` and forking a 128 MiB
  allocation in the subtree triggers OOM (asserted via cgroup events).
- Cleanup on Drop: dropping the `Subtree` removes the directory if no
  pids remain.
- Orphan cleanup: a stale subtree with no live pids is removed by
  `cleanup_orphan_subtree`.
