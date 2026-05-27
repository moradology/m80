# Compromised VMM Network Boundary

## Behavior

m80's network modes describe guest networking first. Only some modes imply a
private network namespace for the Firecracker VMM process.

- `NoEgress` gives the guest no NIC and mutates no host iptables rules. It asks
  the selected launch path to create a private empty network namespace for the
  Firecracker process: `PrivateNetwork=yes` on the systemd path, or
  `m80-jailer-harden --new-net-ns` on the wrapper path. If the VMM process is
  compromised, ordinary host-namespace TCP/UDP access is outside the
  `NoEgress` boundary because the VMM is not in the host network namespace.
- `AllowOutbound` is guest egress policy: m80 realizes a run-root bridge,
  host/vmm veth pair, and VMM-local TAP/bridge path for guest packets and admits
  destinations through `m80-net-outbound`. The Firecracker VMM process joins the
  m80-owned network namespace through jailer `--netns`; host firewall rules key
  on the host-side veth, not on a host-visible TAP.
- `JoinNetns` is the explicit VMM network namespace boundary. The caller owns
  namespace creation, interfaces, routes, firewall policy, and teardown; m80
  validates the namespace path and passes it to the official Firecracker jailer
  as `--netns`. Defense tests for this mode should assert placement in the
  requested namespace, not assume a particular egress policy inside it.

This remains narrower than "a compromised VMM has no network" for all modes:
`AllowOutbound` intentionally gives the guest an egress data path, but the VMM
process is no longer left in the host network namespace for that owned path.

## Evidence

- `crates/m80-net-mode/tests/resolve.rs::compromised_vmm_network_boundary_is_explicit_join_netns_only`
  pins the resolver-level shape: `NoEgress` and `AllowOutbound` do not carry a
  caller-provided namespace path, while `JoinNetns` does.
- `crates/m80-firecracker/tests/end_to_end_real_kvm.rs::end_to_end_real_kvm_join_netns_places_firecracker_in_requested_namespace`
  proves the real-KVM `JoinNetns` launch path places Firecracker in the caller
  namespace.
- `crates/m80-firecracker/tests/egress_none_real_kvm.rs::no_egress_firecracker_runs_in_private_netns`
  proves the real-KVM `NoEgress` launch path places Firecracker outside the
  host network namespace.
- `crates/m80-firecracker/tests/egress_outbound_real_kvm.rs` covers the
  `AllowOutbound` guest egress and peer-rejection battery; the private VMM
  namespace placement is part of the OutboundNat launch plan.
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

The battery can assert host-namespace TCP/UDP isolation for `NoEgress` and
`AllowOutbound` through network namespace placement. For `AllowOutbound`, guest
egress must still be tested separately because the namespace contains the
guest-facing TAP and veth data path.
