# Jailer Cgroup Deferral

## Behavior

`m80-jailer` does not forward Firecracker jailer placement flags
`--cgroup <file>=<value>` or `--parent-cgroup`.

m80's cgroup owner is `m80-cgroup`. The launch path first materializes and
starts the jailed Firecracker process, then `m80-cgroup::Subtree::create`
creates `/sys/fs/cgroup/m80-firecracker/<vm-id>`, enables the required v2
controllers, writes limits, tunes OOM scoring, and enrolls the jailer and
Firecracker pids. This keeps the kernel cgroup mutation surface in one crate
instead of splitting placement between Firecracker's official jailer and m80's
post-launch cgroup code.

`JailerConfig::new_cgroup_ns` is orthogonal. It asks `m80-jailer-harden` to
enter a private cgroup namespace before execing the official jailer so the
jailed process sees a hidden hierarchy. It does not create, limit, or place the
host cgroup leaf.

## NUMA And Cpuset Placement

Use the m80 cgroup path, not Firecracker jailer `--cgroup`, for CPU placement.
Set `SandboxConfig::cpuset_cpus` on the orchestrator side; when
`CgroupMode::UnifiedV2` is active, `m80-firecracker` passes that value to
`m80-cgroup::Limits::cpuset_cpus`. `Subtree::create` writes the leaf
`cpuset.cpus` value before PID enrolment and inherits `cpuset.mems` from the
nearest non-empty ancestor, falling back to `cpuset.mems.effective` on sparse
cgroup-v2 hosts.

For NUMA placement, run m80 under a parent cgroup whose `cpuset.mems` already
names the desired NUMA node set, then set `SandboxConfig::cpuset_cpus` to CPUs
from that node set. m80 intentionally does not expose a separate `cpuset.mems`
API today; memory-node selection remains an operator/topology decision encoded
in the parent cgroup.

## Relation To `--cgroup-version`

`--cgroup-version 2` is the only official-jailer cgroup flag m80 forwards.
That flag selects the cgroup hierarchy dialect used internally by Firecracker's
official jailer. It does not place the process in a host cgroup and does not
replace `m80-cgroup`'s leaf creation, limit writes, or PID enrolment.

If the question is which cgroup hierarchy dialect Firecracker's official
jailer should assume, `docs/behaviors/jailer/cgroup-version.md` supersedes this
document and `--cgroup-version 2` is the relevant flag. If the question is
placement, NUMA, or resource limits, this document owns the answer:
`--cgroup` and `--parent-cgroup` stay absent, and `m80-cgroup` owns the leaf.

## Tests

- `crates/m80-jailer/tests/jailer/no_cgroup_flag.rs::minimal_launch_emits_no_official_jailer_cgroup_placement_flags`
  asserts the minimal launch argv contains neither `--cgroup` nor
  `--parent-cgroup` placement flags.
- `crates/m80-jailer/src/materialized_tests.rs::launch_with_minimal_config_omits_official_jailer_cgroup_placement_flags`
  pins the same argv contract at the private launch-construction layer.
- `crates/m80-jailer/src/materialized_tests.rs::launch_with_cgroup_version_v2_passes_official_jailer_flag`
  and `crates/m80-jailer/tests/jailer/cgroup_version.rs` pin the separate
  `--cgroup-version 2` behavior.
- `crates/m80-cgroup` tests pin the post-launch owner side: cpuset validation,
  sparse `cpuset.cpus`/`cpuset.mems` inheritance, and PID enrolment after
  limit writes.
