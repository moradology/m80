# Warm-Pool Runtime Control And Counters

Captured by beads `m80-r4308.14`, `m80-r4308.4`, `m80-qei44.2`, and
`m80-28x1s.3`.

`WarmPool::new` rejects a `target_ready` value larger than the backend
`max_concurrent_vms` admission limit. Runtime resize uses the same bound:
`WarmPool::set_target_ready` returns `ConfigError::InvalidValue` rather than
starting a fill loop that can only churn on admission refusal.

`WarmPool::set_target_ready` changes only the VM-mechanics target. Growing
starts the existing background fill path. Shrinking discards surplus ready
slots, but it does not force-drop leased slots. Leased slots settle naturally
when the caller returns or discards the lease. `WarmPoolSnapshot::target_ready`
reflects the new target immediately, even while fill or shrink work is still
settling. Pools constructed with `WarmPoolCpuAllocator` cannot grow beyond the
initial allocator capacity because the disjoint `cpuset.cpus` ranges are
allocated from the construction-time target.

`WarmPoolSnapshot` exposes both point-in-time slot counts and monotonic
counters:

- `ready`, `filling`, and `leased` describe current slot state.
- `discarded` counts slots discarded since pool creation, including lease
  discards, dead ready-slot discards, fill failures, shrink discards, and
  shutdown discards.
- `consecutive_fill_errors` tracks the current backoff streak and resets on a
  successful fill.
- `fill_attempts_total` and `fill_failures_total` distinguish an idle pool from
  a pool that cannot replenish.
- `lease_acquired_total` and `lease_returned_total` expose lease flow without
  requiring callers to infer it from current counts.

Evidence:

- `crates/m80-firecracker/tests/warm_pool.rs` covers construction validation,
  runtime grow/shrink behavior, dead ready-slot discard, fill-failure counters,
  and shutdown discard accounting.
- `crates/m80-cli/src/cmds/warm/status.rs` renders the expanded snapshot fields
  in both JSON and human status output.
