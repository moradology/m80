# `m80-net-mode`

A tiny crate (~150 LOC) holding the one-bit decision: "does this VM get
network egress, or doesn't it?"

## Reason for being

The decision is small but it's the seam between two very different
implementations: `NoEgress` (no NIC, no iptables, no privilege needed)
and `OutboundNat` (3,500 LOC of bridge/tap/IP/MAC/iptables policy in
`m80-net-outbound`). Splitting the resolver from both lets:

- Callers express *intent* (`NetworkPolicy::AllowOutbound { exceptions: ... }`)
  without dragging the OutboundNat implementation crate.
- `m80-firecracker` decide which network crate to wire in based on the
  resolved mode without a feature flag.
- The default-deny invariant (workspaces default to `NoEgress`) live in
  one place and be tested in isolation.

It's small. It earns its keep by being the API border between
"caller-friendly intent" and "implementation-heavy realization."

## Black-box contract

- The default for any `NetworkPolicy` constructor is `NoEgress`. Egress
  must be opted into explicitly.
- `resolve(policy: &NetworkPolicy) -> VmNetworkMode` is a pure function
  with no I/O. It does not consult the host, the network, or any
  external state.
- The resolver does not validate exception CIDRs beyond syntax. Range
  validation, max-prefix-length enforcement, and host-route conflict
  checks live downstream in `m80-net-outbound` (which is where they can
  be tested against a real host).
- IPv6 is rejected at the resolver level: `NetworkPolicy::AllowOutbound`
  with an IPv6 entry in `exceptions` returns `ResolveError::Ipv6Unsupported`.
  v0.1 is IPv4-only by design; this is the single place that gate lives.

## Public surface

- `NetworkPolicy { NoEgress, AllowOutbound { exceptions: Vec<Ipv4Net> } }`.
- `VmNetworkMode { NoEgress, OutboundNat { plan: OutboundIntent } }`.
- `OutboundIntent` — pre-validated payload for `m80-net-outbound`:
  exceptions, optional gateway override, etc.
- `resolve(&NetworkPolicy) -> Result<VmNetworkMode, ResolveError>`.
- `ResolveError`: `Ipv6Unsupported`, `MalformedCidr(String)`.

## Non-goals

- **No iptables.** That's `m80-net-outbound`.
- **No DNS.** That's also `m80-net-outbound`.
- **No "should I network" inference.** The caller decides.

## Dependencies

- `serde`, `thiserror`.
- (no other m80 crates).

## Tests

- Default: a freshly-constructed `NetworkPolicy` resolves to `NoEgress`.
- Round-trip: every legal `NetworkPolicy` round-trips through serde
  unchanged.
- IPv6 refusal: any IPv6 entry in `exceptions` returns the typed error.
- CIDR malformedness: non-parseable strings produce `MalformedCidr` with
  the offending input.
