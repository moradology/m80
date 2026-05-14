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

## Public surface and black-box contract

See rustdoc for full signatures.

| Public item | Contract |
| --- | --- |
| `Subtree::probe()` | Confirms the host runs cgroup v2 unified hierarchy and returns `CgroupError::UnsupportedHostMode` on hybrid or v1-only hosts. The first process-local call reads `/proc/mounts` and root `cgroup.subtree_control`; later calls replay the cached result. This is a precondition check; callers gate cgroup usage on a single probe result. |
| `Subtree::create(vm_id, &MaterializedJail, &JailedFirecracker, &Limits)` | Materializes `/sys/fs/cgroup/m80-firecracker/<vm_id>/`, enables `cpu`, `memory`, `pids`, and requested `cpuset`/`io` controllers on the shared ancestor chain once per process, applies limits, tunes OOM score, and only then enrols the deduped jailer/firecracker pid set. `jailer_pid = 0` is skipped as the `new_pid_ns` sentinel. Sparse `cpuset.cpus` and `cpuset.mems` leaf files inherit the nearest non-empty ancestor value before PID enrolment or return `SparseInheritedFile`. |
| `Subtree::leaf_path(vm_id)` | Pure no-I/O helper returning the expected public leaf path for triage and inspection. |
| `Subtree::Drop` | Writes `1` to leaf `cgroup.kill` when that kernel file is present, waits briefly for `cgroup.procs` to drain, then removes the leaf cgroup directory with `rmdir`; if any best-effort teardown step fails, logs through `tracing` and never panics. |
| `cleanup_orphan_subtree(vm_id)` | Startup helper for stale leaves from prior crashed runs. Missing leaves are accepted; non-empty `cgroup.procs` leaves are logged and preserved; empty leaves are removed. |
| `Limits { cpu_max, memory_max, pids_max, cpuset_cpus, io_max, io_weight, oom_score_adj }` | Caller-provided limit profile. `None` fields leave existing controller values alone; empty `io_max` leaves device throttles alone. Setting `cpuset_cpus` writes the leaf `cpuset.cpus` before PID enrolment. |
| `Limits::preset()` | One full CPU, 1.5 GiB memory, 128 pids, no explicit cpuset pin, and `/proc/<pid>/oom_score_adj = 500`. Device-specific `io.max` rows remain caller-provided. |
| `CpuMax` | Field type for `Limits::cpu_max`; either a concrete `(quota_us, period_us)` pair or `Max`. |
| `IoMax { major, minor, rbps, wbps, riops, wiops }` | Field type for `Limits::io_max`; one cgroup v2 `io.max` throttle row for a host-specific block device. Its `Display` implementation renders the kernel file row. |
| `CgroupError` | `UnsupportedHostMode`, `ControllerNotEnabled(&'static str)`, `SparseInheritedFile(&'static str)`, `InvalidLimit { field, value }`, and `Io { path, source }`. |

## Non-goals

- **No cgroup v1 support.** v0.1 is unified-v2 only.
- **No `devices.allow` / `devices.deny` file interface.** Those are cgroup
  v1-era device-controller files. m80's v2-only device-access hardening stays
  with jailer mount/device-node policy unless a future BPF device-controller
  surface is deliberately added.
- **No hidden limit selection.** `Limits` is an input. `m80-firecracker`
  intentionally starts from `Limits::preset()` when `CgroupMode::UnifiedV2`
  is enabled, then applies caller-owned overrides such as `cpuset_cpus`.
- **No standalone construction helpers.** `CpuMax` and `IoMax` are field types;
  callers use enum variants and struct literals directly.
- **No metrics scraping.** Reading `cpu.stat` / `memory.stat` belongs in `m80-observability`.

## Dependencies

- `m80-jailer` public jail/process types.
- `serde`, `thiserror`, `tracing`.
- Requires `/sys/fs/cgroup` at runtime; tests that touch the real
  cgroup hierarchy are `#[ignore]` and run with `sudo`.

## Tests

- Unit (in-crate): mounts-string parsing, process-local probe caching, `Limits`/`CpuMax`/`IoMax` JSON round-trip, error `Display` shape, cpuset override validation, two-phase limit-before-enrolment ordering, recursive subtree-control writes without repeated descendant controller reads, process-local subtree-control priming, sparse cpuset inheritance, and an ignored real-host Drop-with-live-procs kill regression.
- `tests/integration_root.rs` — `#[ignore]` real-host probe, pids.max
  enforcement, cpuset affinity enforcement, and io.max throughput enforcement;
  `sudo cargo test -p m80-cgroup -- --ignored`.
