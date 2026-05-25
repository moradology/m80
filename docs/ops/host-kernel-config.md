# Host Kernel Configuration

This runbook records host kernel posture that m80 depends on or reports. It is
operator-owned: m80 validates launch-critical gates, but it does not rewrite the
host kernel command line, load persistent modules, or remount global kernel
filesystems.

## Microcode

Install Intel or AMD CPU microcode packages and make sure they load early in
boot. m80 does not read
`/sys/devices/system/cpu/cpu0/microcode/version`; it relies on the host's CPU
vulnerability sysfs rows and the operator's OS image management.

## Kernel Command Line

Firecracker production guidance recommends a quiet host kernel command line for
latency-sensitive fleets. A typical latency-oriented setting is:

```text
quiet loglevel=1
```

This host setting is independent of m80's guest kernel command line. m80's
stripped guest kernel already uses quiet guest logging and keeps
`8250.nr_uarts=1` so serial diagnostics remain available when the guest fails.
There is no host-side `8250.nr_uarts` requirement for m80.

## Required Modules

Make the runtime modules survive reboot:

```text
kvm_intel
kvm_amd
vhost_vsock
tun
bridge
br_netfilter
nf_conntrack
```

Load the vendor KVM module that matches the host CPU; do not load both vendor
modules on purpose. A persistent modules file can live under
`/etc/modules-load.d/`, for example:

```sh
cat <<'EOF' | sudo tee /etc/modules-load.d/m80.conf
vhost_vsock
tun
bridge
br_netfilter
nf_conntrack
EOF
```

`m80 preflight` checks the modules and device files it needs before launch.

## cgroup favordynmods

On Linux 6.1 and newer, m80 emits an awareness row for the cgroup v2
`favordynmods` regression class. Operators should choose one mitigation and
record it with host build evidence:

- remount the cgroup v2 hierarchy with `favordynmods`, or
- boot with `kvm.nx_huge_pages=never`.

If the warning is accepted for a trusted deployment, set
`M80_SKIP_CHECK_CGROUP_FAVORDYNMODS=1` to record an explicit operator skip.

## Related Tuning

Transparent hugepages, CPU governors, KVM halt-polling, and network sysctls are
covered in [`host-tuning.md`](host-tuning.md). Do not claim a tuning win without
a before/after m80 measurement on the target host class.
