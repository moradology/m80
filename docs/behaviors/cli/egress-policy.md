# CLI Egress Policy

Behavior capture for bead `m80-lt15.11`.

## Default

`m80 run` defaults to outbound egress. This is a CLI product decision: the
process-wrapper facade should feel like running a host process unless the caller
chooses a smaller world view.

The lower-level library default remains conservative. `SandboxConfig::default`
uses `NetworkPolicy::NoEgress`; the CLI explicitly converts the omitted
`--egress` flag into `NetworkPolicy::AllowOutbound { exceptions: [] }`.

Verification:
`crates/m80-cli/tests/parse_args.rs::parse_run_defaults_to_process_wrapper_contract`
and
`crates/m80-cli/src/cmds/tests.rs::run_egress_mode_maps_to_sandbox_network_policy`.

## Lockdown

`m80 run --egress none -- <program>` selects `NetworkPolicy::NoEgress`. That
means the sandbox has no guest NIC and does not ask OutboundNat to create a
bridge, TAP, DNS config, sysctl setting, or iptables rules.

The old VM-ish spelling `noegress` is rejected. The accepted values are exactly
`none` and `outbound`.

Verification:
`crates/m80-cli/tests/parse_args.rs::parse_run_visibility_and_exec_options`,
`crates/m80-cli/tests/parse_args.rs::parse_run_rejects_removed_noegress_spelling`,
and
`crates/m80-cli/src/cmds/tests.rs::run_egress_mode_maps_to_sandbox_network_policy`.

## Outbound

`m80 run --egress outbound -- <program>` selects
`NetworkPolicy::AllowOutbound { exceptions: [] }`. During backend admission and
launch, that policy resolves to OutboundNat, which owns deterministic bridge/TAP
setup, DNS injection, iptables policy, and ownership-aware cleanup.

If the host cannot support outbound setup, the wrapper fails explicitly through
the normal m80 error path. There is no silent fallback from requested outbound
egress to no-egress.

Verification:
`crates/m80-cli/src/cmds/tests.rs::run_egress_mode_maps_to_sandbox_network_policy`
and the OutboundNat evidence under `docs/behaviors/network-outbound-nat/`.
The real-KVM positive path is pinned by
`crates/m80-firecracker/tests/egress_outbound_real_kvm.rs::{allow_outbound_resolves_external_dns,allow_outbound_reaches_external_http}`.
Those ignored tests require `M80_RUN_EXTERNAL_NETWORK_E2E=1` because they touch
the public network.

## Allowlist Follow-Up

`--allow-host <host>` and `--allow-cidr <cidr>` are reserved shapes. They parse
so help and examples can name the intended policy vocabulary, but today they
exit with feature-gap code 7 before backend work starts.

Hostname allowlists are deferred to `m80-exy.9`. The deferred design follows the
SmolVM DNS-proxy lesson: hostname policy needs a DNS-aware path, not a weak
attempt to precompute CDN IP ranges at parse time.

Verification:
`crates/m80-cli/tests/feature_gap_smoke.rs::run_egress_allowlist_is_explicit_feature_gap`.
