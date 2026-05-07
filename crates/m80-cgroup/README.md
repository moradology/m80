# `m80-cgroup`

Per-VM cgroup v2 subtree under `/sys/fs/cgroup/m80-firecracker/<vm-id>`.
Enables cpu/memory/pids controllers, enrols `firecracker_pid`, cleans up
on Drop.

## Reason for being

cgroup v2 enforcement is small (~300 LOC) but easy to get subtly wrong:
the wrong subtree path leaks state across VMs; failing to write
`cgroup.subtree_control` first makes the controller files invisible;
writing `O_TRUNC` to a virtual file returns EINVAL on some kernels.
A separate crate gives that surface one test boundary and lets
`m80-firecracker` skip cgroups cleanly on hosts without unified-v2.

## Black-box contract

- `Subtree::probe()` confirms the host runs cgroup v2 unified hierarchy;
  returns `CgroupError::UnsupportedHostMode` on hybrid or v1-only hosts.
  This is a **precondition check**, not an idempotent guard — callers are
  expected to gate cgroup usage on a single probe result.
- `Subtree::create(vm_id, &MaterializedJail, &JailedFirecracker)`
  materializes the leaf directory under
  `/sys/fs/cgroup/m80-firecracker/<vm_id>/`, enables the `cpu`, `memory`,
  and `pids` controllers in `cgroup.subtree_control`, and enrols the
  deduped jailer + firecracker pid set. `jailer_pid = 0` is the m80-jailer
  sentinel for "official jailer parent already exited" in `new_pid_ns` mode
  and is skipped. All three steps happen atomically from the OS's perspective;
  partial state is cleaned up on error.
- `Subtree::leaf_path(vm_id)` is a pure, no-I/O path helper that returns
  the expected leaf directory for a given VM id. Callers may use it for
  triage or inspection without holding a `Subtree` handle.
- **Cleanup on drop.** `Subtree::Drop` removes the leaf cgroup directory
  with `rmdir`. If `rmdir` fails (e.g., processes are still enrolled),
  the failure is logged via `tracing`; the drop never panics.
- **v2 unified hierarchy only.** No cgroup v1 or hybrid-mode support in
  v0.1. Any host that does not present a pure unified hierarchy fails at
  `probe()` time, not at `create()` time.

## Public surface

See rustdoc for full signatures.

- `Subtree::probe()` — preflight gate; returns `UnsupportedHostMode` on hybrid/v1 hosts.
- `Subtree::create(vm_id, &MaterializedJail, &JailedFirecracker)` — materialize the leaf and enrol the deduped jailer/firecracker pid set.
- `Subtree::apply_limits(&Limits)` — write per-controller files; `None` fields leave existing values alone.
- `Subtree::leaf_path(vm_id)` — pure path helper for the public cgroup layout.
- `Subtree::Drop` — `rmdir` the leaf if empty; logs on failure, never panics.
- `cleanup_orphan_subtree(vm_id)` — startup helper for stale leaves from prior crashed runs.
- `Limits { cpu_max: Option<CpuMax>, memory_max: Option<u64>, pids_max: Option<u32> }`.
- `Limits::m80_default()` — one full CPU, 1.5 GiB memory, 128 pids.
- `CgroupError`: `UnsupportedHostMode`, `ControllerNotEnabled(&'static str)`, `Io { path, source }`.

## Non-goals

- **No cgroup v1 support.** v0.1 is unified-v2 only.
- **No hidden limit selection inside `Subtree::apply_limits`.** `Limits` is an input.
  `m80-firecracker` intentionally passes `Limits::m80_default()` when
  `CgroupMode::UnifiedV2` is enabled.
- **No metrics scraping.** Reading `cpu.stat` / `memory.stat` belongs in `m80-observability`.

## Dependencies

- `thiserror`, `tracing`.
- No other m80 crates.
- Requires `/sys/fs/cgroup` at runtime; tests that touch the real
  cgroup hierarchy are `#[ignore]` and run with `sudo`.

## Tests

- Unit (in-crate): mounts-string parsing, `Limits`/`CpuMax` JSON round-trip, error `Display` shape.
- `tests/integration_root.rs` — `#[ignore]` real-host probe; `sudo cargo test -p m80-cgroup -- --ignored`.
