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
