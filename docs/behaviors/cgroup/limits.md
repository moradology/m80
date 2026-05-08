# Cgroup v2 Limits — Behaviors

## cpu-max

The system writes `cpu.max` as `"<quota_us> <period_us>\n"` when the caller
supplies `CpuMax::Quota { quota_us, period_us }`. When `CpuMax::Max` is
supplied, the system writes `"max\n"` (no quota).

The predecessor default was `100000 100000` (one full CPU). m80 captures that in
`Limits::m80_default()`, and `m80-firecracker` applies that profile when
`CgroupMode::UnifiedV2` is enabled.

Source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/cgroup.rs`
`CPU_MAX_VALUE` (line 17); `materialize_jailed_cgroup` (line 94).

Test: `crates/m80-cgroup/tests/cgroup/limits.rs::cpu_max_one_cpu`.

## memory-pids-max

The system writes `memory.max` as `"<bytes>\n"` and `pids.max` as
`"<count>\n"` for the corresponding `Limits` fields.

The predecessor defaults are captured in `Limits::m80_default()`: 1 610 612 736
bytes (1.5 GiB) for memory and 128 for pids. `m80-firecracker` applies that
profile for unified-v2 cgroups.

Source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/cgroup.rs`
`MEMORY_MAX_VALUE_BYTES` (line 18), `PIDS_MAX_VALUE` (line 19);
`materialize_jailed_cgroup` (lines 95–99).

Test: `crates/m80-cgroup/tests/cgroup/limits.rs::memory_and_pids_max`.
Test: `crates/m80-cgroup/tests/integration_root.rs::cgroup_pids_max_enforced_against_fork_bomb`
(#[ignore]) creates a real cgroup leaf with `pids.max = 128`, enrolls a
waiting fork workload, and asserts the kernel rejects the next fork with
`EAGAIN`, `pids.current` reaches 128, and `pids.events max` increments.
Test: `crates/m80-firecracker/tests/cgroup_memory_oom_real_kvm.rs::cgroup_memory_limit_oom_kills_workload`
(#[ignore]) launches a real VM with unified-v2 cgroups enabled, verifies
`memory.max` equals the default profile, forces guest tmpfs writes beyond that
host cap, and asserts `memory.events` records OOM enforcement. Because the
cgroup constrains the Firecracker process, the user-visible failure is VM
teardown/protocol loss rather than a trustworthy guest workload exit code.

## cpuset-pinning

When the caller configures a leaf `cpuset.cpus`, the kernel constrains enrolled
process affinity to that CPU set. A process inside the leaf cannot widen its
effective `Cpus_allowed_list` beyond the cgroup cpuset.

Test: `crates/m80-cgroup/tests/integration_root.rs::cgroup_cpuset_pinning_actually_constrains_affinity`
(#[ignore]) creates a real cgroup leaf pinned to one effective host CPU,
enrolls a workload that tries to widen its scheduler affinity, and asserts
`/proc/<pid>/status` still reports only the pinned CPU.

## io-and-oom-defaults

The default unified-v2 profile enables the `io` controller and writes
`io.weight` as `default 100`. `io.max` rows are caller-provided `IoMax`
entries because the block-device major/minor pair is host-specific.

The same default profile writes `/proc/<pid>/oom_score_adj = 500` for the
deduped jailer/firecracker pids before enrolment so the jailed VM is preferred
over host control processes under memory pressure. Setting `oom_score_adj =
None` leaves the process default untouched.

Test: `crates/m80-cgroup/tests/cgroup/limits.rs::memory_and_pids_max`.
Test: `crates/m80-cgroup/tests/cgroup/limits.rs::io_max_json_round_trip`.
Test: `crates/m80-cgroup/src/lib.rs::tests::io_max_formats_v2_row`.
Test: `crates/m80-cgroup/src/lib.rs::tests::io_weight_range_is_kernel_bounded`.
Test: `crates/m80-cgroup/tests/integration_root.rs::cgroup_io_max_throttles_disk_writes`
(#[ignore]) creates a real cgroup leaf with `io.max wbps=1048576` for the
temp-dir backing device, enrolls a direct-write workload before it starts, and
asserts measured throughput stays below 1.5 MiB/s while `io.stat` records
writes for that device.

## device-controller-boundary

m80 is cgroup-v2-only. Literal `devices.allow` / `devices.deny` files are a
cgroup v1 device-controller interface, so they are not part of this crate's v2
surface. Device exposure remains enforced by the jailer mount/device-node
contract unless a future BPF cgroup-device controller is added deliberately.
The v2 I/O noisy-neighbor controls are `io.max` and `io.weight`.

## mode-gate

The orchestrator gates cgroup enforcement on `M80_CGROUP_MODE=unified-v2`. When
the mode is `disabled` (or absent), `Subtree::create` is not called and no
cgroup files are written. The `Subtree::probe()` call in preflight surfaces
`CgroupError::UnsupportedHostMode` when the host does not have unified cgroup v2,
allowing the orchestrator to switch to `disabled` mode.

m80 does not gate inside `m80-cgroup` itself — the crate assumes it is only
called when cgroup mode is appropriate. The gate lives in `m80-firecracker`
where `BackendConfig::cgroup_mode` is checked.

Source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/cgroup.rs`
`FirecrackerCgroupMode` (lines 21–46); `materialize_jailed_cgroup` (lines 79–81).

Test: `crates/m80-cgroup/tests/cgroup/limits.rs::disabled_mode_skips`.
Test: `crates/m80-cgroup/tests/probe_no_v2.rs::v1_mounts_returns_unsupported`.
