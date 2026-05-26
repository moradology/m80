# Outbound Isolation Coverage

OutboundNat is IPv4-only. Policy setup disables IPv6 on the owned run-root
bridge and host-side veth before installing any per-VM iptables rules. Guest
PID-1 network configuration also disables IPv6 router advertisements and
link-local addressing.

IPv4 ICMP is rejected in the per-VM filter chain before the final public IPv4
ACCEPT rule. DNS resolver traffic, private exceptions, permanent deny CIDRs,
ICMP rejection, and public IPv4 acceptance remain one ordered
`iptables-restore -w --noflush` batch.

Routed outbound traffic enters a per-VM filter chain only when the packet
traverses the m80 bridge from that VM's host-side veth bridge port and source
guest `/32`. The host-side veth is the host-visible ingress port for the
private VMM namespace topology. Matching the shared bridge without the physdev
port would allow a sibling VM to spoof another guest's source IPv4 and enter
the wrong per-VM chain.

L2 sibling isolation is pinned by bridge-port isolation on each host-side veth.
Failed per-VM setup after bridge creation rolls back the guest-IP claim, VM
state file, host-side veth, VMM namespace, and now-unused bridge state.

## Evidence

- `crates/m80-net-outbound/tests/network-outbound-nat/iptables.rs::sysctls_set_ip_forward_and_disable_ipv6_before_rules`
- `crates/m80-net-outbound/tests/network-outbound-nat/iptables.rs::icmp_rejected_before_default_accept`
- `crates/m80-net-outbound/tests/network-outbound-nat/iptables.rs::forward_inserts_route_guest_through_filter_chain`
- `crates/m80-net-outbound/tests/network-outbound-nat/setup.rs::bridge_setup_is_idempotent_with_matching_state`
- `crates/m80-net-outbound/tests/network-outbound-nat/setup.rs::failed_launch_after_bridge_cleans_bridge`
- `crates/m80-firecracker/tests/egress_outbound_real_kvm.rs::allow_outbound_rejects_peer_guest_ipv4_on_shared_bridge`
- `crates/m80-firecracker/tests/egress_outbound_real_kvm.rs::allow_outbound_rejects_external_icmp`
