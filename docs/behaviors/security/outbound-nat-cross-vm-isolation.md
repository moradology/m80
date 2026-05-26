# OutboundNat Cross-VM Isolation

OutboundNat treats the shared run-root bridge as host plumbing, not as a
trusted isolation primitive. Per-VM isolation is enforced in three places:

- each TAP bridge port is marked isolated through rtnetlink after it is
  attached to the run-root bridge;
- preflight requires `br_netfilter` plus
  `net.bridge.bridge-nf-call-iptables=1` before OutboundNat can launch, so
  bridge traffic traverses iptables;
- IPv6 is disabled on the owned bridge and host-side veth interfaces before
  firewall rules are installed;
- outbound FORWARD ingress matches the m80 bridge plus
  `--physdev-in <host_veth>` and the guest `/32`, so a sibling VM cannot spoof
  another guest source address and enter that VM's filter chain. The host-side
  veth is the host-visible per-VM ingress port for the private VMM namespace
  topology.

The IPv4 NAT policy remains per-VM and comment-tagged for cleanup. Bridge
egress replies still match the bridge output path plus conntrack state.

Tests:

- `crates/m80-net-outbound/src/link_ops.rs::tests::tap_bridge_lifecycle_orders_link_operations`
- `crates/m80-net-outbound/tests/network-outbound-nat/setup.rs::bridge_setup_is_idempotent_with_matching_state`
- `crates/m80-preflight/src/checks_tests.rs::preflight_missing_br_netfilter_typed`
- `crates/m80-preflight/src/checks_tests.rs::bridge_nf_call_iptables_requires_enabled_sysctl`
- `crates/m80-net-outbound/tests/network-outbound-nat/iptables.rs::sysctls_set_ip_forward_and_disable_ipv6_before_rules`
- `crates/m80-net-outbound/tests/network-outbound-nat/iptables.rs::forward_inserts_route_guest_through_filter_chain`
