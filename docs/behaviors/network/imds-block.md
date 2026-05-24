# IMDS Link-Local Block

m80 rejects guest traffic to `169.254.0.0/16` before the terminal outbound
accept in every outbound-NAT per-VM filter chain. This range covers IPv4
link-local metadata endpoints such as AWS, GCP, and Azure IMDS.

The block is installed by `m80-net-outbound`: `permanent_deny_cidrs()` includes
`169.254.0.0/16`, and `ensure_filter_chain_rules()` emits that CIDR as a
`REJECT` rule in the per-VM chain reached from the VM tap's `FORWARD` path.
The terminal per-VM `ACCEPT` is appended after the permanent-deny list, so the
IMDS reject remains reachable.

This is a pre-NAT guarantee. The packet reaches `FORWARD` before
`POSTROUTING`, so IMDS-destined traffic is rejected before the NAT table's
`MASQUERADE` rule can apply.

DNS admission has the same boundary: `is_admitted_dns_resolver()` rejects
link-local resolver addresses, so there is no DNS-specific accept path that can
allow `169.254.0.0/16` traffic before the permanent deny.

This behavior covers IPv4 outbound NAT only. IPv6 link-local metadata traffic
(`fe80::/10`) is outside the current contract because IPv6 outbound networking
is unsupported and fails with `NetError::Ipv6Unsupported`.

Regression tests:

- `imds_cidr_rejected_in_filter_chain`
- `imds_reject_precedes_terminal_accept_in_filter_chain`
