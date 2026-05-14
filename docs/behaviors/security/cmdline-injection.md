# Kernel Cmdline Injection Guard

Bead: `m80-8emae.1`.

m80 treats the Firecracker boot command line as security-critical launch
mechanics. The command line always contains m80's selected base tokens,
`init=/m80-guestd`, `m80.workspace=0|1`, and `m80.rootfs=<format>` before any
caller-provided extras. `SandboxConfig::boot_args` is append-only and cannot
replace PID 1.

Admission rejects caller extras that contain ASCII control characters or attempt
to set m80-owned tokens:

- `init=`
- `m80.workspace=`
- `m80.rootfs=`
- `rootfstype=`

Generated PID-1 network tokens are also validated as single command-line
tokens before Firecracker receives them. A token containing whitespace fails
closed instead of becoming two kernel arguments.

`NetworkPolicy::JoinNetns` uses `MacAddr` for `NetnsSpec::guest_mac`. The type
accepts only strict six-octet colon-separated MAC addresses, so a caller cannot
place whitespace or a second `init=` token in the guest MAC field that is later
rendered as `m80.net.mac=<value>`.

`NetnsSpec::dns_resolvers` is `Vec<Ipv4Addr>`, so DNS cmdline tokens are also
constructed from parsed IPv4 addresses rather than caller strings. m80 has no
separate hostname cmdline surface; callers that need any extra kernel token must
go through `SandboxConfig::boot_args`, which is append-only and reserved-key
checked.

Workflow guardrail:

- `AGENTS.md` contains an "Untrusted-input-to-kernel-primitive checklist" for
  kernel cmdline tokens, mount paths, capability sets, BPF/seccomp filters, and
  cgroup file contents.
- The audit-sweep ineligibility classifier in `AGENTS.md` includes kernel
  cmdline construction and string fields that flow to mount, exec, capability,
  seccomp, BPF, or cgroup payloads.

Tests:

- `crates/m80-firecracker/tests/security/cmdline_injection.rs::boot_args_init_override_fails_before_admission_permit`
- `crates/m80-firecracker/tests/security/cmdline_injection.rs::boot_args_workspace_marker_injection_is_rejected`
- `crates/m80-firecracker/tests/security/cmdline_injection.rs::boot_args_rootfs_marker_injection_is_rejected`
- `crates/m80-firecracker/tests/security/cmdline_injection.rs::boot_args_rootfstype_override_is_rejected`
- `crates/m80-firecracker/tests/security/cmdline_injection.rs::boot_args_control_char_injection_is_rejected`
- `crates/m80-firecracker/tests/security/cmdline_injection.rs::join_netns_guest_mac_space_injection_is_rejected`
- `crates/m80-firecracker/tests/security/cmdline_injection.rs::join_netns_dns_resolver_cmdline_injection_is_rejected`
- `crates/m80-firecracker/tests/security/cmdline_injection.rs::sandbox_config_has_no_hostname_cmdline_surface`
- `crates/m80-firecracker/src/preboot_boot_arg_tests.rs::boot_args_caller_tokens_append_after_m80_tokens`
- `crates/m80-firecracker/src/preboot_boot_arg_tests.rs::boot_args_reject_init_override`
- `crates/m80-firecracker/src/preboot_boot_arg_tests.rs::boot_args_reject_generated_token_whitespace`
- `crates/m80-net-mode/tests/resolve.rs::netns_spec_rejects_guest_mac_with_whitespace`
