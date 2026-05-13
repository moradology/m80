# Outbound NAT Iptables Policy

## IP Forward

The host policy phase invokes `sysctl -w net.ipv4.ip_forward=1` before any
guest FORWARD or NAT POSTROUTING rule is installed. If the sysctl command
fails, policy installation aborts and no per-VM filter, FORWARD, or NAT rules
are appended after that failure.

After the sysctl succeeds, missing policy rules are installed through one
`iptables-restore -w --noflush` batch. The apply path still creates or reuses the
per-VM chain first, rejects foreign rules in that owned chain, and lists the
per-VM chain, FORWARD, and NAT POSTROUTING for missing-rule detection so
reapplying the same policy does not duplicate existing rules.

Source: predecessor
`crates/sandbox/agent-sandbox-firecracker/src/network.rs::ensure_ipv4_forwarding`
lines 1491-1497, called from `apply_outbound_nat_policy_with_host` around
line 1091.

Verification:
`crates/m80-net-outbound/tests/network-outbound-nat/iptables.rs::sysctl_ip_forward_set_before_rules`
and `::sysctl_failure_aborts_before_rule_install`.
`crates/m80-net-outbound/tests/network-outbound-nat/iptables.rs::policy_rules_install_with_one_iptables_restore_batch`
and `::iptables_restore_failure_aborts_policy_install`.

## Filter Chain

Each VM owns one filter chain named `tfw` plus the first 12 hex characters of
`sha256(run_dir)`. The policy phase checks `iptables -w -t filter -S <chain>`
first and creates the chain with `iptables -w -t filter -N <chain>` only when
the check fails.

Source: predecessor `outbound_nat_filter_chain` lines 1499-1501 and
`ensure_iptables_chain` lines 1746-1780.

Verification:
`crates/m80-net-outbound/tests/network-outbound-nat/iptables.rs::per_vm_filter_chain_named_tfw_plus_12_hex`.

## DNS Accept

For each admitted resolver in `network-state.json`, the per-VM filter chain
gets two ACCEPT rules: UDP port 53 to that resolver and TCP port 53 to that
resolver. Every rule carries the per-VM m80 comment.

Source: predecessor `ensure_outbound_nat_filter_chain_rules` lines 1519-1560.

Verification:
`crates/m80-net-outbound/tests/network-outbound-nat/iptables.rs::dns_accept_per_admitted_resolver_udp_and_tcp`.

## DNS Reject

After the resolver-specific ACCEPT rules, the per-VM filter chain rejects all
other UDP and TCP traffic to destination port 53. This keeps guest DNS pinned
to the admitted upstream resolver set.

Source: predecessor `ensure_outbound_nat_filter_chain_rules` lines 1562-1581.

Verification:
`crates/m80-net-outbound/tests/network-outbound-nat/iptables.rs::reject_other_port_53`.

## Private Exceptions

After the DNS gate, the per-VM filter chain accepts each caller-provided
bounded private IPv4 exception CIDR with `-d <destination> -j ACCEPT`, tagged
with the per-VM comment.

Source: predecessor `ensure_outbound_nat_filter_chain_rules` lines 1583-1600
and private-exception validation lines 913-949.

Verification:
`crates/m80-net-outbound/tests/network-outbound-nat/iptables.rs::bounded_private_exceptions_accepted`.

## Permanent Deny

After private exceptions, the per-VM filter chain rejects the permanent-deny
CIDR set: `0.0.0.0/8`, `10.0.0.0/8`, `100.64.0.0/10`, `127.0.0.0/8`,
`169.254.0.0/16`, `172.16.0.0/12`, `192.168.0.0/16`,
`192.0.2.0/24`, `198.18.0.0/15`, `198.51.100.0/24`,
`203.0.113.0/24`, `224.0.0.0/4`, `240.0.0.0/4`, and the specific bridge
CIDR. The bridge CIDR is intentionally redundant with the `172.16.0.0/12`
umbrella because it remains correct if the allocator later moves.

Source: predecessor `denied_outbound_cidrs` lines 1721-1744.

Verification:
`crates/m80-net-outbound/tests/network-outbound-nat/iptables.rs::permanent_deny_list_includes_bridge_cidr`.

## Default Accept

The per-VM filter chain ends with a final comment-tagged `-j ACCEPT` rule.
Traffic that survives DNS rejection, private exception handling, and the
permanent-deny list is allowed to reach public IPv4 destinations.

Source: predecessor `ensure_outbound_nat_filter_chain_rules` lines 1621-1627.

Verification:
`crates/m80-net-outbound/tests/network-outbound-nat/iptables.rs::default_accept_after_deny_list`.

## Forward Entries

The policy phase inserts three filter/FORWARD rules at index 1:
`-i <bridge> -s <guest_ipv4>/32 -j <chain>` routes guest egress through the
per-VM filter chain, `-o <bridge> -d <guest_ipv4>/32 -j REJECT` blocks new
inbound traffic, and
`-o <bridge> -d <guest_ipv4>/32 -m conntrack --ctstate RELATED,ESTABLISHED -j ACCEPT`
permits replies. Because all three use `-I ... 1`, the final effective order
keeps RELATED/ESTABLISHED above the inbound reject.

The FORWARD entries key on the Linux bridge interface, not the TAP device.
Once the TAP is enslaved to the bridge, routed guest traffic reaches the host
FORWARD chain as bridge ingress/egress; the guest `/32` keeps the rule scoped
to one VM.

Source: predecessor `ensure_forwarding_entry_rules` lines 1630-1695.

Verification:
`crates/m80-net-outbound/tests/network-outbound-nat/iptables.rs::forward_inserts_route_guest_through_filter_chain`.

## NAT Masquerade

The policy phase appends one NAT POSTROUTING rule:
`-s <guest_ipv4>/32 -j MASQUERADE`, tagged with the per-VM comment. The host
therefore source-NATs guest egress onto whichever host interface owns the
default route.

Source: predecessor `ensure_nat_masquerade_rule` lines 1697-1719.

Verification:
`crates/m80-net-outbound/tests/network-outbound-nat/iptables.rs::nat_postrouting_masquerade_for_guest_source`.
