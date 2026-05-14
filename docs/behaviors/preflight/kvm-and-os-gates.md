# KVM and OS Gates

`m80-preflight` performs host gates before boot artifacts are accepted or a VM is
launched. The live `run()` path checks these once per preflight invocation and
fails closed on the first missing capability.

## Linux Host

The OS gate reads `uname -s` and accepts only `Linux`. macOS, Windows, and other
hosts return `PreflightError::UnsupportedHostPlatform`.

## Host Kernel Floor

The host kernel release from `uname -r` must parse as Linux 6.1 or newer. Older
or unparseable releases return `PreflightError::HostKernelUnsupported` before
KVM, binary, or artifact checks run.

## KVM Device

`/dev/kvm` must exist and be writable by the current process. A missing device
returns `PreflightError::KvmUnavailable`; permission denial returns
`PreflightError::KvmNotWritable`.

## Kernel Modules And Vsock

The host must have both `tap` and `bridge` loaded in `/proc/modules`. m80 reports
missing modules with `PreflightError::KernelModulesMissing` and does not attempt
to load them.

The host must also expose vhost-vsock for Firecracker's host↔guest control
channel. Preflight accepts either a loaded `vhost_vsock` module in
`/proc/modules` or an existing `/dev/vhost-vsock` device, because some kernels
expose the device without a loadable module entry. If neither signal is present,
preflight returns `PreflightError::VsockUnavailable`.

The host must expose TUN for TAP-backed outbound networking. Preflight accepts
either a loaded `tun` module in `/proc/modules` or an existing `/dev/net/tun`
device. If neither signal is present, preflight returns
`PreflightError::TunUnavailable`.

The host must expose `nf_conntrack` for outbound NAT. Preflight accepts either a
loaded `nf_conntrack` module in `/proc/modules` or `/sys/module/nf_conntrack`,
which covers kernels that expose the module state outside the loaded-module
text file. If neither signal is present, preflight returns
`PreflightError::NfConntrackUnavailable`.

The host must expose `br_netfilter` and set
`net.bridge.bridge-nf-call-iptables=1` before OutboundNat is accepted. The
module makes Linux bridge traffic visible to iptables, and the sysctl enables
that path. If the module is absent, preflight returns
`PreflightError::BridgeNetfilterUnavailable`; if the sysctl is not `1`,
preflight returns `PreflightError::BridgeNfCallIptablesDisabled`.

## Cgroup Mode

When effective config requests `cgroup_mode = "unified-v2"`, preflight calls
`m80-cgroup::Subtree::probe()` before binary discovery or artifact validation.
If the host is not in unified cgroup v2 mode, preflight returns
`PreflightError::CgroupV2Unavailable`. When effective config requests
`cgroup_mode = "disabled"`, preflight skips the cgroup v2 probe.

Standalone `m80_preflight::run()` derives this setting from `M80_CGROUP_MODE`
and defaults to `unified-v2`, matching `m80-firecracker` config defaults.
Callers that already loaded effective config pass
`HostFeaturePreflightConfig` to `run_with_configs`.

## Jailer Identity Gate

The effective `jail_uid` and `jail_gid` must resolve through host passwd and
group lookup before launch work begins. The standalone `run()` path reads
`M80_JAIL_UID` and `M80_JAIL_GID`, defaulting both to `3000`; callers with
already-loaded config pass those fields through `HostFeaturePreflightConfig`.

Invalid numeric values return `PreflightError::InvalidJailIdentity`. Missing
host user or group entries return `PreflightError::JailIdentityUnavailable`.
Preflight does not create the user/group and does not fall back to another id.

## CPU Microcode Reporting

Preflight reads CPU0 microcode `version` and `processor_flags` from sysfs when
available and emits a non-blocking `CPU microcode` row. Missing sysfs rows are
reported as `unavailable`; m80 does not infer microcode level from vulnerability
status text.

## CPU Vulnerability Gate

Preflight reads selected files under
`/sys/devices/system/cpu/vulnerabilities/`. `mds` and `l1tf` are hard gates:
when the kernel reports `Vulnerable`, preflight returns
`PreflightError::CpuVulnerabilityDetected` before launch work begins.

The remaining tracked files (`spectre_v2`, `retbleed`, `tsx_async_abort`,
`srbds`, `mmio_stale_data`, and `gather_data_sampling`) are advisory rows.
`Vulnerable`, unreadable, unavailable, and unclassified statuses are preserved
in the report detail so operators can make the host-placement decision with the
kernel's actual text visible.

Operators may set `M80_SKIP_CHECK_VULNERABILITIES=1` to bypass the hard gate
after accepting the side-channel risk. Other values do not disable the gate.

## Privilege Gate

m80 accepts exactly two startup privilege shapes:

- `geteuid() == 0`
- non-root with every capability in `REQUIRED_CAPABILITIES` present in the
  effective Linux capability set

`REQUIRED_CAPABILITIES` is `CAP_NET_ADMIN`, `CAP_SYS_ADMIN`, `CAP_MKNOD`,
`CAP_CHOWN`, `CAP_FOWNER`, `CAP_KILL`, `CAP_SETUID`, `CAP_SETGID`, and
`CAP_SETPCAP`. `CAP_SETPCAP` is required only before the jailer hardening
wrapper boundary so it can prune the official jailer's bounding set; the wrapper
removes it before exec. Missing capabilities return
`PreflightError::PrivilegeUnavailable`.

There is no passwordless-sudo fallback and no per-call privilege shim in m80.
Operators must run as root, set capabilities on the binary, or provide the
capabilities through the container runtime.

## Evidence

- `crates/m80-preflight/src/checks.rs`
- `crates/m80-preflight/src/lib.rs::classify_privilege`
- `crates/m80-preflight/tests/preflight/kvm_and_os_gates.rs`
- `crates/m80-preflight/src/checks_tests.rs::host_kernel_floor_rejects_old_release`
- `crates/m80-preflight/src/checks_tests.rs::jailer_identity_requires_existing_user`
- `crates/m80-preflight/src/checks_tests.rs::cpu_microcode_reports_version_and_flags`
- `crates/m80-preflight/src/checks_tests.rs::preflight_missing_vsock_module_typed`
- `crates/m80-preflight/src/checks_tests.rs::preflight_missing_tun_module_typed`
- `crates/m80-preflight/src/checks_tests.rs::preflight_missing_nf_conntrack_typed`
- `crates/m80-preflight/src/checks_tests.rs::preflight_missing_br_netfilter_typed`
- `crates/m80-preflight/src/checks_tests.rs::bridge_nf_call_iptables_requires_enabled_sysctl`
- `crates/m80-preflight/src/checks_tests.rs::preflight_cgroup_v2_unavailability_typed`
- `crates/m80-preflight/src/checks_tests.rs::cpu_vulnerability_mds_vulnerable_fails_closed`
- `crates/m80-preflight/src/checks_tests.rs::cpu_vulnerability_scan_reports_all_configured_files`
- `crates/m80-preflight/tests/preflight/kvm_and_os_gates.rs::privilege_gate_rejects_missing_setpcap_for_jailer_wrapper_pruning`
