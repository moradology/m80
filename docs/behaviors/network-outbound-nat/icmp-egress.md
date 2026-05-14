# ICMP Egress Rejection

Behavior capture for bead `m80-8emae.26`.

## Contract

OutboundNat admits DNS to discovered resolvers, rejects all other DNS, allows
bounded private exceptions, rejects the permanent-deny CIDR set, and then
rejects IPv4 ICMP before the final public-IPv4 ACCEPT.

The ICMP rule is:

```text
-p icmp -m comment --comment <owned-comment> -j REJECT
```

It is installed in the per-VM filter chain through the same
`iptables-restore -w --noflush` batch as the rest of the policy. The rule must
appear before the default ACCEPT, otherwise echo request payloads can bypass the
DNS/CIDR policy and act as a covert exfiltration path.

IPv6 ICMP is not separately admitted here. The current contract remains no IPv6
for OutboundNat; IPv6 disablement and bridge-port isolation are tracked by the
separate network hardening beads.

## Verification

- `crates/m80-net-outbound/tests/network-outbound-nat/iptables.rs::icmp_rejected_before_default_accept`
- `crates/m80-net-outbound/tests/network-outbound-nat/iptables.rs::policy_rules_install_with_one_iptables_restore_batch`
- `crates/m80-firecracker/tests/egress_outbound_real_kvm.rs::allow_outbound_rejects_external_icmp`
  (ignored; requires KVM, CAP_NET_ADMIN, and `M80_RUN_EXTERNAL_NETWORK_E2E=1`)
