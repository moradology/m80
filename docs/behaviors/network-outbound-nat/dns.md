# Outbound NAT DNS Discovery

## Discovery

The DNS discovery phase invokes `resolvectl dns` first and parses IPv4
tokens from stdout. If that command is missing, fails, or yields no admitted
public IPv4 resolvers, discovery falls back to parsing `nameserver` lines
from `/etc/resolv.conf`. If both sources yield no admitted resolvers, the
phase returns `NoUsableDnsResolvers`.

Source: predecessor
`crates/sandbox/agent-sandbox-firecracker/src/network.rs::discover_dns_resolvers`
lines 1413-1432, `parse_resolv_conf` lines 1434-1446, and
`parse_resolver_tokens` lines 1448-1456.

Verification:
`crates/m80-net-outbound/tests/network-outbound-nat/dns.rs::resolvectl_then_resolv_conf_fallback`,
`::resolvectl_admitted_resolvers_win_without_resolv_conf`, and
`::no_usable_dns_resolvers_errors`.

## Admission

Every candidate resolver passes through `is_admitted_dns_resolver`. The
admitted set is public IPv4 only. Unspecified, loopback, private, link-local,
multicast, broadcast, and documentation addresses are rejected.

Source: predecessor `is_admitted_dns_resolver` lines 1467-1478.

Verification:
`crates/m80-net-outbound/tests/network-outbound-nat/dns.rs::is_admitted_dns_resolver_admits_public_ipv4_only`.

## Reject Categories

Resolver admission also rejects carrier-grade NAT `100.64.0.0/10`,
benchmark `198.18.0.0/15`, leading-zero `0.0.0.0/8`, and
reserved/multicast `224.0.0.0/3` ranges. These checks intentionally go
beyond the standard `Ipv4Addr` predicates so the admitted set does not widen
silently when a range is not covered by a single standard helper.

Source: predecessor `is_admitted_dns_resolver` lines 1479-1487.

Verification:
`crates/m80-net-outbound/tests/network-outbound-nat/dns.rs::reject_cgn_benchmark_reserved_resolvers`.
