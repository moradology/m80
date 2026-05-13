# Host Tuning

This document collects host-level settings that affect m80 latency or density
but are not part of m80's VM mechanics. m80 reports these settings so operators
can choose a deployment posture explicitly.

## Transparent Hugepages

Linux exposes the current transparent hugepage policy at:

```sh
cat /sys/kernel/mm/transparent_hugepage/enabled
```

The selected policy is wrapped in brackets, for example:

```text
always [madvise] never
```

Firecracker guest RAM is anonymous memory in the Firecracker child process.
m80 cannot call `madvise(MADV_HUGEPAGE)` inside that child process. In
`madvise` mode, guest RAM therefore does not receive THP unless Firecracker
itself requests it. In `always` mode, the kernel may back that anonymous memory
with transparent hugepages without reserving explicit hugepages ahead of time.

Latency-priority hosts should evaluate:

```sh
echo always | sudo tee /sys/kernel/mm/transparent_hugepage/enabled
```

Density-priority hosts may prefer the distro default if THP compaction causes
tail jitter under load. Treat this as an operator policy, not an m80 launch
precondition.

Transparent hugepages and explicit hugepages are different operator choices.
THP `always` mode is best-effort and needs no reservation: the kernel may
promote Firecracker's anonymous guest-memory pages when conditions allow it.
Explicit Firecracker `huge_pages` is the guaranteed path: it requires reserving
host hugepages with settings such as `vm.nr_hugepages=N`, and that reservation
reduces the flexibility of host memory allocation.

## KVM Halt Polling

KVM exposes halt-poll settings through module parameters:

```sh
cat /sys/module/kvm/parameters/halt_poll_ns
cat /sys/module/kvm/parameters/halt_poll_ns_grow
cat /sys/module/kvm/parameters/halt_poll_ns_shrink
cat /sys/module/kvm/parameters/lapic_timer_advance
cat /sys/module/kvm_intel/parameters/enable_preemption_timer
```

`halt_poll_ns` is the maximum time KVM will busy-poll before parking a vCPU
after guest `HLT`. A larger value can reduce wakeup latency during boot and
short command bursts, but it burns more host CPU while the vCPU is otherwise
idle. The `grow` and `shrink` parameters control KVM's adaptive halt-poll
window.

Latency-priority hosts can evaluate:

```sh
echo 400000 | sudo tee /sys/module/kvm/parameters/halt_poll_ns
```

Density-priority hosts should keep the distro default or lower it if idle CPU
burn matters more than a few milliseconds of wakeup latency. m80 reports the
current value but does not enforce it.

Timer behavior is also affected by LAPIC timer settings. On hosts exposing
`/sys/module/kvm/parameters/lapic_timer_advance`, m80 reports the current
advance value in the same preflight row. Some Intel kernels also expose
`/sys/module/kvm_intel/parameters/enable_preemption_timer`; when present, that
value is reported because it affects whether LAPIC timer advance is relevant.

This is visibility only. Do not claim a latency win from changing these values
without a before/after cold-launch artifact.
