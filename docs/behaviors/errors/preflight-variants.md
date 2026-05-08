# Preflight Error Variants

## binary-not-found

`m80-preflight` rejects a missing Firecracker binary with the typed
`PreflightError::FirecrackerBinaryNotFound` variant. The rendered message tells
the operator to install Firecracker at the default path or set
`M80_FIRECRACKER_BIN`.

predecessor source: `crates/sandbox/agent-sandbox-firecracker/src/errors.rs:152-153`.

Test: `crates/m80-preflight/tests/error_hints.rs::firecracker_binary_not_found_has_hint`.

## kvm-unavailable

`m80-preflight` rejects a missing `/dev/kvm` with
`PreflightError::KvmUnavailable { path }` and rejects a present but unwritable
device with `PreflightError::KvmNotWritable { path }`. Both variants carry the
offending path so the CLI message and JSON detail identify the host device that
failed.

predecessor source: `crates/sandbox/agent-sandbox-firecracker/src/errors.rs:206-210`.

Test: `crates/m80-preflight/tests/error_hints.rs::kvm_unavailable_has_hint` and
`crates/m80-preflight/tests/error_hints.rs::kvm_not_writable_has_hint`.

## vsock-unavailable

`m80-preflight` rejects a host without vhost-vsock support with
`PreflightError::VsockUnavailable`. This is distinct from the generic kernel
module list because Firecracker's vsock device may be present as
`/dev/vhost-vsock` even when there is no loadable `vhost_vsock` entry in
`/proc/modules`.

Test:
`crates/m80-preflight/src/checks.rs::tests::preflight_missing_vsock_module_typed`
and `crates/m80-preflight/tests/error_hints.rs::vsock_unavailable_has_hint`.

## tun-unavailable

`m80-preflight` rejects a host without TUN support with
`PreflightError::TunUnavailable`. This is distinct from the generic kernel
module list because TUN may be available through `/dev/net/tun` even when the
`tun` module is not listed in `/proc/modules`.

Test:
`crates/m80-preflight/src/checks.rs::tests::preflight_missing_tun_module_typed`
and `crates/m80-preflight/tests/error_hints.rs::tun_unavailable_has_hint`.

## unsupported-host

`m80-preflight` rejects non-Linux hosts with
`PreflightError::UnsupportedHostPlatform { actual }`. The `actual` field is the
platform string reported by `uname -s`.

predecessor source: `crates/sandbox/agent-sandbox-firecracker/src/errors.rs:204`.

Test: `crates/m80-preflight/tests/error_hints.rs::unsupported_host_platform_has_hint`.

## first-line-sizing

m80 does not carry predecessor's fixed first-line sizing gate. `SandboxConfig`
keeps `vcpu_count` and `mem_size_mib` as caller-configurable lifecycle inputs,
with defaults of 1 vCPU and 1024 MiB when omitted. Invalid machine sizing is
therefore surfaced by the Firecracker client/resource call that rejects it, not
by a preflight-only `UnsupportedFirstLineVmSizing` compatibility variant.

predecessor source:
`crates/sandbox/agent-sandbox-firecracker/src/errors.rs:226-229`.

Test:
`crates/m80-firecracker/src/launch.rs::tests::machine_config_honors_caller_sizing`
and
`crates/m80-firecracker/src/launch.rs::tests::machine_config_uses_default_sizing_when_omitted`.

## priv-jailer-unavail

m80 acquires privilege once at process startup. A process that is neither root
nor capability-bearing fails preflight with
`PreflightError::PrivilegeUnavailable { missing_caps }`; there is no per-call
`sudo -n` jailer fallback and no compatibility
`PrivilegedJailerLaunchUnavailable` shim. If the already-admitted process later
hits a jailer filesystem or chroot failure, that failure surfaces through
`JailerError` with the path-specific cause.

predecessor source:
`crates/sandbox/agent-sandbox-firecracker/src/errors.rs:221-224` and
`foundation.rs:847`.

Test: `crates/m80-preflight/tests/error_hints.rs::privilege_unavailable_has_hint`.
