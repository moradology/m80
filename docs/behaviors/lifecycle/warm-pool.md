# Warm Pool Allocation

## allocation-contract

The system exposes `WarmPool` as the clean stateless allocation path. A
pool owns pre-restored, guestd-ready slots and `try_lease()` hands out one
ready `WarmLease` without issuing a cold boot, synchronous restore, or
readiness exec on the request path. A restored slot enters `Ready` when
its configured `ready_probe` request completes with status `Completed`
and exit code `0`; the probe response is the publication gate.

An empty pool returns `FcError::PoolEmpty`. This is intentional: callers
must size and prefill the pool, or handle unavailability explicitly.

When `WarmPoolConfig::cpu_allocator` is set, the pool derives exactly
`target_ready` contiguous CPU ranges from `first_cpu` and `cpus_per_slot`.
Each filling, ready, or leased slot owns one range and passes it through
`SandboxConfig::cpuset_cpus`, so unified-v2 launch writes the assigned range to
that VM's leaf `cpuset.cpus`. Refill waits for a range to be released instead
of duplicating a range while all configured ranges are leased. If the host does
not expose enough CPUs for the requested range set, `WarmPool::new` fails with a
typed config error. With no allocator, warm slots inherit the parent cpuset.

**m80 tests.**

- `crates/m80-firecracker/tests/warm_pool.rs::reset_evidence_requires_every_input`
- `crates/m80-firecracker/tests/warm_pool.rs::reset_evidence_does_not_infer_from_partial_truth`
- `crates/m80-firecracker/src/warm_pool/cpu_allocator.rs::tests::builds_disjoint_slot_ranges`
- `crates/m80-firecracker/src/warm_pool/cpu_allocator.rs::tests::rejects_insufficient_host_cpus`
- `crates/m80-firecracker/tests/warm_pool.rs::warm_pool_cpuset_allocator_assigns_disjoint_concurrent_slots` (ignored real-KVM/root integration)
- `crates/m80-firecracker/tests/warm_pool.rs::warm_pool_allocates_pre_restored_slot_and_refills_after_discard` (ignored real-KVM integration)

## reset-evidence

The system returns a VM to `Ready` only when complete
`BlankVmResetEvidence` is present:

- ownership marker and lease
- boot identity
- no workspace id attached
- no run id attached
- empty or template-equal guest workspace
- clean run-root surface
- clean diagnostics
- post-reset guestd probe

Any missing or failed input produces `BlankVmResetDecision::Discard`.
The first implementation does not infer reuse from liveness, process
handles, socket existence, metrics presence, or a clean-looking tree, so
leased slots are discarded and replaced.

## benchmark

`crates/m80-firecracker/benches/warm_pool_allocation_latency.rs` measures
`try_lease -> first exec` separately from background refill wait. It
reports idle and loaded P50/P95/max/mean values and writes JSON when
`M80_WARM_POOL_BENCH_OUTPUT` is set.

Real-KVM run on 2026-05-05 with N=50 and one ready slot:

| load | allocation P50 | allocation P95 | allocation max | refill P95 |
|---|---:|---:|---:|---:|
| idle | 20.957 ms | 21.053 ms | 21.070 ms | 1261.443 ms |
| loaded (`stress-ng --cpu $(nproc)`) | 21.005 ms | 623.409 ms | 1334.006 ms | 1535.415 ms |

The loaded P95 includes restored-vsock local-init retry delay. The pool still
keeps the request path off direct snapshot restore most of the time, but a
saturated host can burn part or all of the bounded open+send retry budget.

Raw artifacts:

- `docs/behaviors/lifecycle/warm-pool-allocation-latency-idle.json`
- `docs/behaviors/lifecycle/warm-pool-allocation-latency-loaded.json`

## transport-send-retry

When Firecracker's restored-vsock local-init path transiently fails during
`CONNECT`, or accepts `CONNECT` but fails before the request frame is delivered,
host open/send can return `HandshakeFailed` or `BrokenPipe`.
`RunningSandbox::exec` retries only that open+send failure for a fixed 2.5 s
budget. It does not retry receive-side errors, because a response failure may
mean the guest already ran the command.
