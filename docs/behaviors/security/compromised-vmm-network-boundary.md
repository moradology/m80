# Compromised VMM Network Boundary

## Behavior

m80's network modes describe guest networking first. They do not all imply a
private network namespace for the Firecracker VMM process.

- `NoEgress` gives the guest no NIC and mutates no host iptables rules. It does
  not create or join a private network namespace for the Firecracker process.
  If the VMM process is compromised, ordinary host-namespace TCP/UDP operations
  are outside the current `NoEgress` promise. The Layer 2 network battery must
  focus on privileged network operations blocked by jailer hardening and
  capability drop, such as raw sockets, packet sockets, and netlink mutation.
- `AllowOutbound` is guest egress policy: m80 realizes a TAP/bridge/NAT path
  for guest packets and admits destinations through `m80-net-outbound`. It does
  not currently promise that a compromised VMM process is confined to a
  process-private network namespace. Ordinary host-namespace TCP/UDP operations
  are not a valid failure assertion for the defense-in-depth battery.
- `JoinNetns` is the explicit VMM network namespace boundary. The caller owns
  namespace creation, interfaces, routes, firewall policy, and teardown; m80
  validates the namespace path and passes it to the official Firecracker jailer
  as `--netns`. Defense tests for this mode should assert placement in the
  requested namespace, not assume a particular egress policy inside it.

This is deliberately narrower than "a compromised VMM has no network." That
stronger policy would require new launch wiring that creates or joins a
dedicated namespace for `NoEgress` and for m80-owned outbound NAT.

## Evidence

- `crates/m80-net-mode/tests/resolve.rs::compromised_vmm_network_boundary_is_explicit_join_netns_only`
  pins the resolver-level shape: `NoEgress` and `AllowOutbound` do not carry a
  namespace path, while `JoinNetns` does.
- `crates/m80-firecracker/tests/end_to_end_real_kvm.rs::end_to_end_real_kvm_join_netns_places_firecracker_in_requested_namespace`
  proves the real-KVM `JoinNetns` launch path places Firecracker in the caller
  namespace.
- `crates/m80-firecracker/tests/egress_none_real_kvm.rs` proves the guest
  `NoEgress` promise by checking direct IP and DNS failure from inside the
  guest.

## Test Guidance

The Layer 2 network battery should assert:

- raw socket creation fails without `CAP_NET_RAW`;
- packet socket creation fails without `CAP_NET_RAW`;
- netlink link mutation fails without `CAP_NET_ADMIN`;
- binding or addressing a non-existent jailed interface fails with a local
  kernel error.

The battery should not assert that plain TCP listen/connect operations fail in
`NoEgress` or `AllowOutbound` until m80 implements a private VMM netns policy.
