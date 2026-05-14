# Preflight Error Variants

## binary-not-found

`m80-preflight` rejects a missing Firecracker binary with the typed
`PreflightError::FirecrackerBinaryNotFound` variant. The rendered message tells
the operator to install Firecracker at the default path or set
`M80_FIRECRACKER_BIN`.

predecessor source: `crates/sandbox/agent-sandbox-firecracker/src/errors.rs:152-153`.

Test: `crates/m80-preflight/tests/error_hints.rs::firecracker_binary_not_found_has_hint`.

## firecracker-seccomp-filter

`m80-preflight` rejects a missing, non-file, or empty Firecracker advanced
seccomp filter before launch. Missing and non-file paths return
`PreflightError::FirecrackerSeccompFilterNotFound { path }`; empty regular
files return `PreflightError::FirecrackerSeccompFilterEmpty { path }`.

Test:
`crates/m80-preflight/tests/error_hints.rs::firecracker_seccomp_filter_not_found_has_hint`,
`crates/m80-preflight/tests/error_hints.rs::firecracker_seccomp_filter_empty_has_hint`,
`crates/m80-preflight/src/binary.rs::tests::missing_firecracker_seccomp_filter_fails_closed`, and
`crates/m80-preflight/src/binary.rs::tests::empty_firecracker_seccomp_filter_fails_closed`.

## kvm-unavailable

`m80-preflight` rejects a missing `/dev/kvm` with
`PreflightError::KvmUnavailable { path }` and rejects a present but unwritable
device with `PreflightError::KvmNotWritable { path }`. Both variants carry the
offending path so the CLI message and JSON detail identify the host device that
failed.

predecessor source: `crates/sandbox/agent-sandbox-firecracker/src/errors.rs:206-210`.

Test: `crates/m80-preflight/tests/error_hints.rs::kvm_unavailable_has_hint` and
`crates/m80-preflight/tests/error_hints.rs::kvm_not_writable_has_hint`.

## cgroup-v2-unavailable

When effective config requests `cgroup_mode = "unified-v2"`, `m80-preflight`
rejects a host without a unified cgroup v2 hierarchy with
`PreflightError::CgroupV2Unavailable`. This happens before Firecracker launch
phases, not after partial run-root/jailer setup. Invalid `M80_CGROUP_MODE`
values fail with `PreflightError::InvalidCgroupMode { actual }`.

Test:
`crates/m80-preflight/src/checks.rs::tests::preflight_cgroup_v2_unavailability_typed`,
`crates/m80-preflight/src/checks.rs::tests::disabled_cgroup_mode_skips_cgroup_v2_probe`,
`crates/m80-preflight/tests/error_hints.rs::cgroup_v2_unavailable_has_hint`, and
`crates/m80-preflight/tests/error_hints.rs::invalid_cgroup_mode_has_hint`.

## cpu-vulnerability-detected

`m80-preflight` rejects hosts where high-impact CPU side-channel sysfs status
files report `Vulnerable`. The first hard-gated vulnerable row returns
`PreflightError::CpuVulnerabilityDetected { id, detail }`, preserving the
kernel's exact status text in `detail`. The hard-gated files are `mds` and
`l1tf`; other tracked vulnerability files remain advisory report rows.

Operators may bypass the hard gate with `M80_SKIP_CHECK_VULNERABILITIES=1`
after accepting the host side-channel risk.

Test:
`crates/m80-preflight/src/checks.rs::tests::cpu_vulnerability_mds_vulnerable_fails_closed`,
`crates/m80-preflight/src/checks.rs::tests::cpu_vulnerability_medium_vulnerable_is_advisory_row`,
and
`crates/m80-preflight/tests/error_hints.rs::cpu_vulnerability_detected_has_hint`.

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

## nf-conntrack-unavailable

`m80-preflight` rejects a host without conntrack support with
`PreflightError::NfConntrackUnavailable`. m80 needs conntrack for outbound NAT
masquerade behavior, so the absence is reported before launch instead of
surfacing as a later iptables failure.

Test:
`crates/m80-preflight/src/checks.rs::tests::preflight_missing_nf_conntrack_typed`
and
`crates/m80-preflight/tests/error_hints.rs::nf_conntrack_unavailable_has_hint`.

## bridge-netfilter-unavailable

`m80-preflight` rejects a host without `br_netfilter` support with
`PreflightError::BridgeNetfilterUnavailable`. OutboundNat depends on bridge
traffic traversing iptables, otherwise TAP-scoped FORWARD rules do not cover
intra-bridge paths.

Test:
`crates/m80-preflight/src/checks_tests.rs::preflight_missing_br_netfilter_typed`
and
`crates/m80-preflight/tests/error_hints.rs::bridge_netfilter_unavailable_has_hint`.

## bridge-nf-call-iptables-disabled

`m80-preflight` rejects a host where
`/proc/sys/net/bridge/bridge-nf-call-iptables` is not `1` with
`PreflightError::BridgeNfCallIptablesDisabled`.

Test:
`crates/m80-preflight/src/checks_tests.rs::bridge_nf_call_iptables_requires_enabled_sysctl`
and
`crates/m80-preflight/tests/error_hints.rs::bridge_nf_call_iptables_disabled_has_hint`.

## nf-conntrack-capacity-too-low

`m80-preflight` reads `/proc/sys/net/netfilter/nf_conntrack_max` and rejects
hosts whose global conntrack table is below
`2 * M80_MAX_CONCURRENT_VMS * 1000` entries. The default expected concurrency is
8 VMs, matching the backend admission default. Operators either raise
`net.netfilter.nf_conntrack_max` with sysctl or lower `M80_MAX_CONCURRENT_VMS`.

Test:
`crates/m80-preflight/src/checks_tests.rs::nf_conntrack_capacity_requires_expected_vm_headroom`
and
`crates/m80-preflight/tests/error_hints.rs::nf_conntrack_capacity_too_low_has_hint`.

## unsupported-host

`m80-preflight` rejects non-Linux hosts with
`PreflightError::UnsupportedHostPlatform { actual }`. The `actual` field is the
platform string reported by `uname -s`.

predecessor source: `crates/sandbox/agent-sandbox-firecracker/src/errors.rs:204`.

Test: `crates/m80-preflight/tests/error_hints.rs::unsupported_host_platform_has_hint`.

## host-kernel-unsupported

`m80-preflight` rejects Linux kernels older than 6.1, and rejects unparseable
`uname -r` releases, with
`PreflightError::HostKernelUnsupported { actual, minimum }`. This is a host
floor gate, not a Firecracker launch error, so it runs before KVM and artifact
validation.

Test:
`crates/m80-preflight/src/checks_tests.rs::host_kernel_floor_rejects_old_release`,
`crates/m80-preflight/src/checks_tests.rs::host_kernel_floor_rejects_unparseable_release`,
and `crates/m80-preflight/tests/error_hints.rs::host_kernel_unsupported_has_hint`.

## first-line-sizing

m80 does not carry predecessor's fixed first-line sizing gate. `SandboxConfig`
keeps `vcpu_count` and `mem_size_mib` as caller-configurable lifecycle inputs,
with defaults of 1 vCPU and 512 MiB when omitted. Invalid machine sizing is
therefore surfaced by the Firecracker client/resource call that rejects it, not
by a preflight-only `UnsupportedFirstLineVmSizing` compatibility variant.

predecessor source:
`crates/sandbox/agent-sandbox-firecracker/src/errors.rs:226-229`.

Test:
`crates/m80-firecracker/src/preboot.rs::tests::machine_config_honors_caller_sizing`
and
`crates/m80-firecracker/src/preboot.rs::tests::machine_config_uses_default_sizing_when_omitted`.

## jail-identity

The effective jailer identity must be explicit and resolvable on the host.
Invalid numeric `jail_uid`/`jail_gid` input returns
`PreflightError::InvalidJailIdentity { field, value }`. Numeric ids that do not
resolve through passwd or group lookup return
`PreflightError::JailIdentityUnavailable { field, id }`.

Preflight does not create missing identities and does not fall back to root or
the current user.

Test:
`crates/m80-preflight/src/checks_tests.rs::jail_id_parser_rejects_non_u32`,
`crates/m80-preflight/src/checks_tests.rs::jailer_identity_requires_existing_user`,
`crates/m80-preflight/src/checks_tests.rs::jailer_identity_requires_existing_group`,
`crates/m80-preflight/tests/error_hints.rs::invalid_jail_identity_has_hint`, and
`crates/m80-preflight/tests/error_hints.rs::jail_identity_unavailable_has_hint`.

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
