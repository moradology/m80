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

## CPU Governor

Linux exposes the CPU frequency driver and governor for CPU 0 at:

```sh
cat /sys/devices/system/cpu/cpu0/cpufreq/scaling_driver
cat /sys/devices/system/cpu/cpu0/cpufreq/scaling_governor
```

Driver type matters. With `intel_pstate` or `amd_pstate`, a `powersave`
governor is typically a hardware-managed policy: the processor can ramp quickly
through HWP/CPPC, so m80 does not emit an advisory for that combination.

With `acpi-cpufreq`, governor policy is software-managed. A non-`performance`
governor can leave CPUs at a lower frequency until launch work begins, adding
ramp-up latency to boot tail measurements. On latency-priority hosts using
`acpi-cpufreq`, evaluate:

```sh
sudo cpupower frequency-set -g performance
```

Density- or power-priority hosts may keep their distro governor. Treat the
preflight row as visibility, not a launch precondition.

The bench harness may set `CPU_GOVERNOR=performance` for a controlled
measurement run. That knob is for before/after data collection and is not a
production recommendation by itself.

## Run-Root Filesystem

m80 keeps per-VM state, overlay clones, sockets, and diagnostic files under the
configured run-root. The default is `/var/run/m80`; on systemd hosts,
`/var/run` is normally a symlink to `/run`, which is tmpfs.

tmpfs has fast metadata operations but no reflink support. When the run-root
does not support reflinks, the overlay template clone path still works because
storage uses:

```sh
cp --reflink=auto --sparse=always
```

`--reflink=auto` silently falls back to a full byte copy. That is correct for
compatibility, but it can hide avoidable launch-path cost.

Preflight reports a non-blocking `Run-root filesystem` row by creating a small
probe file under the run-root and running:

```sh
cp --reflink=always <probe-src> <probe-dst>
```

For latency-priority cold-launch hosts, prefer a persistent Linux filesystem
with reflinks enabled, such as XFS with reflink support or btrfs. For
short-lived local development, tmpfs may still be a reasonable choice if RAM
pressure and full-copy fallback are acceptable.

Do not put the run-root on a mount that blocks device nodes. Preflight rejects
`nodev` mounts because Firecracker needs device files inside the per-VM chroot.
