# `m80-net-mode`

A tiny crate holding the small network-placement decision: "does this VM get no
network, m80-owned outbound NAT, or a caller-owned network namespace?"

## Reason for being

The decision is small but it's the seam between two very different
implementations: `NoEgress` (no NIC, no iptables, no privilege needed),
`OutboundNat` (3,500 LOC of bridge/tap/IP/MAC/iptables policy in
`m80-net-outbound`), and `JoinNetns` (the caller provisions the namespace plus
a TAP device and the Firecracker jailer joins it). Splitting the resolver from
implementations lets:

- Callers express *intent* (`NetworkPolicy::AllowOutbound { exceptions: ... }`)
  without dragging the OutboundNat implementation crate.
- `m80-firecracker` decide which network crate to wire in based on the
  resolved mode without a feature flag.
- The default-deny invariant (workspaces default to `NoEgress`) live in
  one place and be tested in isolation.

It's small. It earns its keep by being the API border between
"caller-friendly intent" and "implementation-heavy realization."

## Black-box contract

- Callers must pick a `NetworkPolicy` variant explicitly. There is no
  `Default` implementation; `NoEgress` is the conservative choice when in
  doubt. Egress must be opted into explicitly.
- `resolve(policy: &NetworkPolicy) -> VmNetworkMode` is a **pure,
  infallible** function with no I/O. It does not consult the host, the
  network, or any external state.
- The resolver does not validate exception CIDRs beyond what the type
  already guarantees. Range validation, max-prefix-length enforcement, and
  host-route conflict checks live downstream in `m80-net-outbound` (which
  is where they can be tested against a real host).
- `exceptions` is typed `Vec<Ipv4Net>`, so CIDR parsing and IPv6 rejection
  happen at the call-site (CLI, config layer, or proto decode) before
  `resolve` is reached. This crate carries no string-input parser in v0.1;
  a future `NetworkPolicy::parse_from_strings(...)` entrypoint would be
  the place to surface those errors.
- `JoinNetns` carries a namespace fd path, caller-created TAP name, guest MAC,
  guest IPv4/prefix, gateway, and DNS resolvers. The caller owns namespace
  creation, TAP/link setup, routing, firewall rules, and teardown. m80 validates
  and joins the namespace downstream in `m80-jailer`, emits the Firecracker
  network-interface PUT, and passes the static guest network tokens to PID 1.
  The resolver remains pure.
- These modes describe guest networking and VMM placement separately.
  `NoEgress` means the guest gets no NIC; it does not create a private network
  namespace for the Firecracker process. `AllowOutbound` means guest packets go
  through m80-owned TAP/bridge/NAT policy; it likewise does not create a
  process-private VMM namespace. `JoinNetns` is the only mode that explicitly
  places Firecracker in a caller-provided namespace.

## Public surface

- `NetworkPolicy` — caller-facing intent enum: `NoEgress` | `OutboundNat { exceptions: Vec<Ipv4Net> }` | `JoinNetns { netns_path, tap_name, guest_mac, guest_ipv4, gateway_ipv4, dns_resolvers }`.
- `VmNetworkMode` — resolved implementation mode: `NoEgress` | `OutboundNat { exceptions }` | `JoinNetns { netns_path, tap_name, guest_mac, guest_ipv4, gateway_ipv4, dns_resolvers }`.
- `resolve(policy: &NetworkPolicy) -> VmNetworkMode` — pure, infallible resolution.

## Non-goals

- **No iptables.** That's `m80-net-outbound`.
- **No DNS.** Also `m80-net-outbound`.
- **No netns or TAP creation.** Callers provision and own `JoinNetns`
  namespaces, TAP devices, routes, and firewall policy.
- **No compromised-VMM host egress policy.** That stronger defense-in-depth
  boundary belongs in launch/jailer wiring, not in this pure resolver.
- **No "should I network" inference.** The caller decides.
- **No string-input parser.** CIDR parsing and IPv6 rejection live at the
  caller boundary, not here.

## Dependencies

- `ipnet` for `Ipv4Net`.
- No other m80 crates.

## Tests

- `resolve(NoEgress)` returns `VmNetworkMode::NoEgress`.
- `resolve(OutboundNat { exceptions: [] })` returns `VmNetworkMode::OutboundNat`.
- `resolve(OutboundNat { exceptions: [...] })` carries exceptions through unchanged.
- `resolve(JoinNetns { ... })` carries the namespace path and static guest NIC
  contract through unchanged.
- `compromised_vmm_network_boundary_is_explicit_join_netns_only` pins that only
  `JoinNetns` carries a VMM network namespace path.
