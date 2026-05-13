# Cpuset Pinning

## Contract

Per-VM CPU pinning is explicit. `SandboxConfig::cpuset_cpus = None` leaves the
VM leaf cgroup inheriting the parent effective CPU set. Setting
`SandboxConfig::cpuset_cpus = Some(value)` is a caller request to write
`value` to `/sys/fs/cgroup/m80-firecracker/<vm-id>/cpuset.cpus` before the
jailer/firecracker PID set is enrolled.

The cgroup crate treats `cpuset_cpus` as a kernel file value, not an m80
topology language. It accepts only non-empty ASCII digits, commas, and hyphens,
then lets the kernel validate availability and range shape. Invalid local
syntax fails as `CgroupError::InvalidLimit { field: "cpuset_cpus", .. }`;
kernel rejections remain structured I/O errors with the exact file path.

## Warm Pool Allocation

`WarmPoolCpuAllocator` is an optional density knob:

- `first_cpu` names the first host CPU id the pool may use.
- `cpus_per_slot` is the contiguous width assigned to each slot.
- `target_ready` determines how many ranges are reserved.

For `target_ready = 3`, `first_cpu = 0`, and `cpus_per_slot = 2`, the pool
builds `0-1`, `2-3`, and `4-5`. `WarmPool::new` compares the final exclusive
CPU id against `std::thread::available_parallelism()` and fails with
`FcError::Config` if the host cannot satisfy the requested range set.

Each filling, ready, or leased slot owns exactly one range. A refill worker
reserves a range before restore and releases it if restore fails. `WarmLease`
returns the range when the slot is discarded, including one-shot exec cleanup
and attach-drive failure cleanup. If every configured range is already owned by
ready/filling/leased slots, background refill does not create another slot until
a lease releases its range.

## Non-Goals

This does not infer pinning from host topology, NUMA layout, or VM count.
Callers that want pinning must opt in with `WarmPoolCpuAllocator` or by setting
`SandboxConfig::cpuset_cpus` directly.

This does not configure Firecracker's machine CPU template. The Firecracker
machine config remains governed by `vcpu_count`, `mem_size_mib`, `smt = false`,
and optional `cpu_template`; cpuset pinning is host cgroup placement.
