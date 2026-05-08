# KVM and OS Gates

`m80-preflight` performs host gates before boot artifacts are accepted or a VM is
launched. The live `run()` path checks these once per preflight invocation and
fails closed on the first missing capability.

## Linux Host

The OS gate reads `uname -s` and accepts only `Linux`. macOS, Windows, and other
hosts return `PreflightError::UnsupportedHostPlatform`.

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

## Privilege Gate

m80 accepts exactly two startup privilege shapes:

- `geteuid() == 0`
- non-root with every capability in `REQUIRED_CAPABILITIES` present in the
  effective Linux capability set

`REQUIRED_CAPABILITIES` is `CAP_NET_ADMIN`, `CAP_SYS_ADMIN`, `CAP_MKNOD`,
`CAP_CHOWN`, `CAP_FOWNER`, and `CAP_KILL`. Missing capabilities return
`PreflightError::PrivilegeUnavailable`.

There is no passwordless-sudo fallback and no per-call privilege shim in m80.
Operators must run as root, set capabilities on the binary, or provide the
capabilities through the container runtime.

## Evidence

- `crates/m80-preflight/src/checks.rs`
- `crates/m80-preflight/src/lib.rs::classify_privilege`
- `crates/m80-preflight/tests/preflight/kvm_and_os_gates.rs`
- `crates/m80-preflight/src/checks.rs::tests::preflight_missing_vsock_module_typed`
- `crates/m80-preflight/src/checks.rs::tests::preflight_missing_tun_module_typed`
- `crates/m80-preflight/src/checks.rs::tests::preflight_missing_nf_conntrack_typed`
- `crates/m80-preflight/src/checks.rs::tests::preflight_cgroup_v2_unavailability_typed`
