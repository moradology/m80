# DNS Name Allowlist Decision

Behavior bead: `m80-3xwa.5.8`.

## Decision

DNS name allowlisting is not implemented as a guestd verb in the current
`m80-guestd` protocol. The feature belongs to the networking boundary and needs
a staged design before any host/guest wire is added.

`m80-net-outbound` currently enforces CIDR and DNS-resolver policy at the host
iptables/NAT boundary. Name allowlisting is a different contract: it requires a
DNS proxy, resolver configuration, cache/TTL behavior, wildcard semantics,
failure mapping, and interaction with IP-level egress rules.

## Required Shape

A future implementation should be split into explicit pieces:

1. host-side DNS filter in or next to `m80-net-outbound`;
2. exact-match and wildcard-subdomain allowlist semantics;
3. raw DNS packet handling with a two-byte big-endian length prefix if vsock is
   used;
4. blocked queries return NXDOMAIN with RA set;
5. upstream resolver failure returns SERVFAIL;
6. guest resolver integration that routes DNS to the proxy without turning
   `m80-guestd` into a general networking daemon;
7. tests with synthetic DNS packets for A, AAAA, allowed, blocked, wildcard,
   NXDOMAIN, and SERVFAIL cases.

## Current Boundary

For now, m80 continues to expose:

- `NetworkPolicy::NoEgress`;
- `NetworkPolicy::AllowOutbound` / `OutboundNat` with host-side CIDR policy;
- admitted DNS resolver handling in `m80-net-outbound`.

Name-based egress is a future networking feature, not a guest exec or agent
semantic.
