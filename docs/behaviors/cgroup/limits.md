# Cgroup v2 Limits — Behaviors

## cpu-max

The system writes `cpu.max` as `"<quota_us> <period_us>\n"` when the caller
supplies `CpuMax::Quota { quota_us, period_us }`. When `CpuMax::Max` is
supplied, the system writes `"max 100000\n"` (no quota, 100 ms period).

The predecessor default was `100000 100000` (one full CPU). m80 externalizes this
choice into the `Limits` struct — the orchestrator supplies the value.

Source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/cgroup.rs`
`CPU_MAX_VALUE` (line 17); `materialize_jailed_cgroup` (line 94).

Test: `crates/m80-cgroup/tests/cpu_max_format.rs::quota_formats_as_two_numbers`.

## memory-pids-max

The system writes `memory.max` as `"<bytes>\n"` and `pids.max` as
`"<count>\n"` for the corresponding `Limits` fields.

The predecessor defaults were 1 610 612 736 bytes (1.5 GiB) for memory and 128 for
pids. m80 passes these as caller-supplied values in `Limits`.

Source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/cgroup.rs`
`MEMORY_MAX_VALUE_BYTES` (line 18), `PIDS_MAX_VALUE` (line 19);
`materialize_jailed_cgroup` (lines 95–99).

Test: `crates/m80-cgroup/tests/integration_root.rs::apply_limits_writes_controller_files`
(#[ignore]).

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

Test: `crates/m80-cgroup/tests/probe_no_v2.rs::v1_mounts_returns_unsupported`
(verifies the host detection path).
