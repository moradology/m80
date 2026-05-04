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

## Non-goals

- **No iptables.** That's `m80-net-outbound`.
- **No DNS.** Also `m80-net-outbound`.
- **No "should I network" inference.** The caller decides.
- **No string-input parser.** CIDR parsing and IPv6 rejection live at the
  caller boundary, not here.
