# Outbound NAT Collision Detection

Behavior capture for beads `m80-exy.2.1` through `m80-exy.2.3`.

## Guest IPv4

The system scans `<run_root>/*/network-state.json` before realizing a planned
VM network. Any sibling state file whose bridge CIDR matches the planned bridge
CIDR and whose `guest_ipv4` matches the planned guest IPv4 causes a typed
`GuestIpv4Collision` error when the sibling `vm_id` differs from the current VM.

Missing run-roots and VM directories without `network-state.json` are not
collisions. Malformed state is surfaced as `InvalidNetworkState`; the planner
does not silently ignore broken state.

This captures predecessor `reject_guest_ipv4_collision` around lines 415-454.

## Host Route

The system parses `/proc/net/route` and rejects a planned bridge CIDR when it
overlaps any non-default host route. A matching route on the allowed bridge
interface is accepted so idempotent bridge reuse does not reject its own route.

Route addresses and masks are parsed from procfs little-endian hexadecimal
fields. Non-contiguous masks and malformed rows produce `InvalidNetworkState`.

This captures predecessor `reject_host_route_collision` around lines 456-480 and
`parse_proc_net_route` around lines 538-567.

## Hard Error

Collisions are hard errors. The planner does not randomize, retry, or widen the
address pool after either guest-IPv4 or host-route collision. Re-running the
same plan against the same colliding state returns the same typed error.

This preserves deterministic allocation and matches the predecessor Phase 2
contract: collision short-circuits the network plan instead of mutating it.

## Verification

`crates/m80-net-outbound/tests/network-outbound-nat/collision.rs` pins sibling
guest IPv4 collision, same-VM and other-CIDR non-collisions, host-route overlap,
allowed bridge-route reuse, default-route skipping, and the no-random-fallback
hard-error behavior.
