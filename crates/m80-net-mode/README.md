# `m80-net-mode`

A tiny crate (~90 LOC) holding the one-bit decision: "does this VM get
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

## Public surface

- `NetworkPolicy { NoEgress, AllowOutbound { exceptions: Vec<Ipv4Net> } }`.
  Implements `Default` → `NoEgress`.
- `VmNetworkMode { NoEgress, OutboundNat { plan: OutboundIntent } }`.
- `OutboundIntent` — pre-validated payload for `m80-net-outbound`:
  `exceptions: Vec<Ipv4Net>`, `gateway_override: Option<Ipv4Addr>`.
- `resolve(&NetworkPolicy) -> VmNetworkMode` — the single seam for all
  mode decisions. Callers never branch on mode themselves.

## Non-goals

- **No iptables.** That's `m80-net-outbound`.
- **No DNS.** That's also `m80-net-outbound`.
- **No "should I network" inference.** The caller decides.
- **No string-input parser.** CIDR parsing and IPv6 rejection live at the
  caller boundary, not here.

## Dependencies

- `serde`.
- `ipnet` (with `serde` feature).
- (no other m80 crates).

## Tests

Integration tests in `tests/resolve.rs`:

- `default_network_policy_is_noegress` — pins `NetworkPolicy::default()` is `NoEgress`.
- `noegress_resolves_to_noegress` — `NoEgress` in, `NoEgress` out.
- `allow_outbound_with_no_exceptions_resolves_to_outbound_nat` — empty
  `exceptions` still yields `OutboundNat` with `gateway_override: None`.
- `allow_outbound_with_one_cidr_round_trips_through_resolver` — a concrete
  CIDR propagates unchanged.
- `network_policy_roundtrips_through_serde_json_noegress` — serde round-trip.
- `network_policy_roundtrips_through_serde_json_allow_outbound` — serde round-trip.
- `vm_network_mode_roundtrips_through_serde_json_noegress` — serde round-trip.
- `vm_network_mode_roundtrips_through_serde_json_outbound_nat` — serde round-trip.
