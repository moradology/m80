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

Firecracker v1.15.1 does not expose a transparent-hugepage switch for guest
memory. Its `huge_pages` machine-config field is hugetlbfs-backed and uses
reserved 2 MiB hugepages; it is not a `MADV_HUGEPAGE` request.

Latency-priority hosts should evaluate:

```sh
echo always | sudo tee /sys/kernel/mm/transparent_hugepage/enabled
```

Density-priority hosts may prefer the distro default if THP compaction causes
tail jitter under load. Treat this as an operator policy, not an m80 launch
precondition.

Transparent hugepages and explicit hugepages are different operator choices.
THP `always` mode can affect other anonymous host memory, but it is not how m80
requests Firecracker hugepage backing. `m80 run --huge-pages-2m` maps to
Firecracker `huge_pages: "2M"` and requires reserving host hugepages with
settings such as `vm.nr_hugepages=N`; that reservation reduces the flexibility
of host memory allocation.

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

## Network Sysctls

Firecracker's published `v1.15.1`
[`network-performance.md`](https://github.com/firecracker-microvm/firecracker/blob/v1.15.1/docs/network-performance.md)
measurements changed no socket-buffer or other network-related kernel
parameters. Treat that as the default posture for m80 hosts: do not cargo-cult
network sysctls before there is a measured bottleneck.

For single-VM development, moderate throughput, and the default `NoEgress`
mode, distro defaults are usually the right starting point. Under high
throughput, high connection counts, or many concurrent `OutboundNat` VMs,
operators can inspect:

```sh
sysctl net.core.rmem_default
sysctl net.core.rmem_max
sysctl net.core.wmem_default
sysctl net.core.wmem_max
sysctl net.ipv4.tcp_mem
sysctl net.core.somaxconn
sysctl net.netfilter.nf_conntrack_max
```

Increase these only when host evidence points at that class of limit: socket
receive/send buffer pressure, listen backlog saturation, TCP memory pressure, or
conntrack table exhaustion. For Firecracker-specific network ceilings, measure
host-to-guest and guest-to-host throughput with the target image, kernel, and
VM count before changing the host.

m80 does not mutate these sysctls. `m80 preflight` checks launch-critical
network prerequisites, and `m80-net-outbound` owns the bridge, TAP,
per-VM iptables, DNS admission, and cleanup behavior described in
[`crates/m80-net-outbound/README.md`](../../crates/m80-net-outbound/README.md).
Host-wide TCP/socket/conntrack sizing remains operator policy.

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
does not support reflinks, choose the explicit byte-copy overlay clone mode:

```sh
cp --reflink=never --sparse=auto
```

On reflink-capable filesystems, callers may choose the explicit reflink mode:

```sh
cp --reflink=always --sparse=auto
```

If mandatory reflink is rejected at clone time, launch fails with the `cp`
error. `m80-storage` does not retry with byte-copy. The `auto` policy only
probes once, selects either byte-copy or reflink, and then runs the selected
command fail-closed.

Preflight reports a non-blocking `Run-root filesystem` row by creating a small
probe file under the run-root and running:

```sh
cp --reflink=always <probe-src> <probe-dst>
```

For latency-priority cold-launch hosts, prefer a persistent Linux filesystem
with reflinks enabled, such as XFS with reflink support or btrfs. For
short-lived local development, tmpfs may still be a reasonable choice if RAM
pressure and explicit byte-copy are acceptable.

Do not put the run-root on a mount that blocks device nodes. Preflight rejects
`nodev` mounts because Firecracker needs device files inside the per-VM chroot.
