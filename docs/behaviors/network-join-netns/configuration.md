# JoinNetns Configuration

`NetworkPolicy::JoinNetns { spec: NetnsSpec }` launches Firecracker inside a
caller-provided network namespace and attaches a caller-created TAP to the
guest. `NetnsSpec` carries the namespace path, TAP name, typed guest MAC, guest
IPv4/prefix, gateway, and DNS resolvers. The MAC is a strict
`HH:HH:HH:HH:HH:HH` value, so it cannot inject additional kernel-cmdline
tokens when m80 builds the static PID-1 network arguments. m80 does not create
the namespace, add links, configure routes, or install firewall rules for this
mode. The caller owns that setup and teardown.

During launch, `m80-net-mode::resolve` carries the `NetnsSpec` through as
`VmNetworkMode::JoinNetns`. `m80-firecracker` copies the namespace path into
`JailerConfig::netns_path`. `m80-jailer` opens the path with `O_NOFOLLOW`,
verifies the fd is backed by `NSFS_MAGIC`, then passes `--netns <path>` to
Firecracker's official jailer. The official jailer performs `setns(CLONE_NEWNET)`
before execing Firecracker. After the Firecracker API socket is ready,
`m80-firecracker` emits `PUT /network-interfaces/eth0` using the spec's
`tap_name` and `guest_mac`; because Firecracker is already inside the joined
namespace, the TAP name is resolved there. PID 1 configures `eth0` from the static
`m80.net=join_netns` boot tokens.

This mode is separate from `NoEgress` and `AllowOutbound`. `NoEgress` still
creates no NIC. `AllowOutbound` still resolves to m80-owned OutboundNat.
`JoinNetns` is the explicit escape hatch for callers that already have a
namespace with the desired TAP, routes, and network policy.

Tests:
- `crates/m80-net-mode/tests/resolve.rs::join_netns_carries_path_through_resolver`
- `crates/m80-net-mode/tests/resolve.rs::netns_spec_rejects_guest_mac_with_whitespace`
- `crates/m80-jailer/src/materialized.rs::tests::validate_netns_path_rejects_regular_file`
- `crates/m80-jailer/src/materialized.rs::tests::validate_netns_path_rejects_symlink`
- `crates/m80-firecracker/tests/end_to_end_real_kvm.rs::end_to_end_real_kvm_join_netns_places_firecracker_in_requested_namespace`
- `crates/m80-firecracker/tests/join_netns_real_kvm.rs::join_netns_routes_guest_traffic_through_caller_namespace`
