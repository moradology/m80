# `m80-cgroup`

Per-VM cgroup v2 subtree under `/sys/fs/cgroup/m80-firecracker/<vm-id>`.
Enables cpu/memory/pids plus needed io controllers, applies cgroup limits
before PID enrolment, tunes OOM preference, and cleans up on Drop.

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
- `Subtree::create(vm_id, &MaterializedJail, &JailedFirecracker, &Limits)`
  materializes the leaf directory under
  `/sys/fs/cgroup/m80-firecracker/<vm_id>/`, recursively enables the `cpu`,
  `memory`, `pids`, and when requested `io` controllers in ancestor
  `cgroup.subtree_control` files, applies limits, and only then enrols the
  deduped jailer + firecracker pid set. `jailer_pid = 0` is the m80-jailer
  sentinel for "official jailer parent already exited" in `new_pid_ns` mode
  and is skipped.
- Sparse `cpuset.cpus` / `cpuset.mems` leaf files inherit the nearest
  non-empty ancestor value before PID enrolment. If the file exists but no
  ancestor has a value, `SparseInheritedFile` is returned instead of silently
  enrolling into an unusable leaf.
- `Limits::m80_default()` writes one CPU, 1.5 GiB memory, 128 pids, default
  cgroup v2 `io.weight = 100`, and `/proc/<pid>/oom_score_adj = 500`.
  `io.max` rows are caller-configured because the device major/minor is
  host-specific.
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
- `Subtree::create(vm_id, &MaterializedJail, &JailedFirecracker, &Limits)` — materialize the leaf, apply limits, tune OOM score, and enrol the deduped jailer/firecracker pid set.
- `Subtree::apply_limits(&Limits)` — write per-controller files; `None` fields leave existing values alone.
- `Subtree::leaf_path(vm_id)` — pure path helper for the public cgroup layout.
- `Subtree::Drop` — `rmdir` the leaf if empty; logs on failure, never panics.
- `cleanup_orphan_subtree(vm_id)` — startup helper for stale leaves from prior crashed runs.
- `Limits { cpu_max, memory_max, pids_max, io_max, io_weight, oom_score_adj }`.
- `IoMax { major, minor, rbps, wbps, riops, wiops }` — one cgroup v2 `io.max` throttle row.
- `Limits::m80_default()` — one full CPU, 1.5 GiB memory, 128 pids, default io weight 100, OOM score 500.
- `CgroupError`: `UnsupportedHostMode`, `ControllerNotEnabled(&'static str)`, `SparseInheritedFile(&'static str)`, `InvalidLimit { field, value }`, `Io { path, source }`.

## Non-goals

- **No cgroup v1 support.** v0.1 is unified-v2 only.
- **No `devices.allow` / `devices.deny` file interface.** Those are cgroup
  v1-era device-controller files. m80's v2-only device-access hardening stays
  with jailer mount/device-node policy unless a future BPF device-controller
  surface is deliberately added.
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

- Unit (in-crate): mounts-string parsing, `Limits`/`CpuMax`/`IoMax` JSON round-trip, error `Display` shape, two-phase limit-before-enrolment ordering, recursive subtree-control writes, sparse cpuset inheritance, and an ignored real-host Drop-with-live-procs regression.
- `tests/integration_root.rs` — `#[ignore]` real-host probe and pids.max
  enforcement; `sudo cargo test -p m80-cgroup -- --ignored`.
