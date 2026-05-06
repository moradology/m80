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

## Public surface

See rustdoc for full signatures.

- `Subtree::probe()` — preflight gate; returns `UnsupportedHostMode` on hybrid/v1 hosts.
- `Subtree::create(vm_id, &MaterializedJail, &JailedFirecracker)` — materialize the leaf and enrol the deduped jailer/firecracker pid set.
- `Subtree::apply_limits(&Limits)` — write per-controller files; `None` fields leave existing values alone.
- `Subtree::path()` — also persisted to `<run_dir>/cgroup-path.txt`.
- `Subtree::leaf_path(vm_id)` — pure path helper for the public cgroup layout.
- `Subtree::Drop` — `rmdir` the leaf if empty; logs on failure, never panics.
- `cleanup_orphan_subtree(vm_id)` — startup helper for stale leaves from prior crashed runs.
- `Limits { cpu_max: Option<CpuMax>, memory_max: Option<u64>, pids_max: Option<u32> }`.
- `Limits::m80_default()` — one full CPU, 1.5 GiB memory, 128 pids.
- `CgroupError`: `UnsupportedHostMode`, `ControllerNotEnabled(String)`, `Io { path, source }`.

## Non-goals

- **No cgroup v1 support.** v0.1 is unified-v2 only.
- **No hidden limit selection inside `Subtree::apply_limits`.** `Limits` is an input.
  `m80-firecracker` intentionally passes `Limits::m80_default()` when
  `CgroupMode::UnifiedV2` is enabled.
- **No metrics scraping.** Reading `cpu.stat` / `memory.stat` belongs in `m80-observability`.

## Tests

- Unit (in-crate): mounts-string parsing, `Limits`/`CpuMax` JSON round-trip, error `Display` shape.
- `tests/integration_root.rs` — `#[ignore]` real-host probe; `sudo cargo test -p m80-cgroup -- --ignored`.
