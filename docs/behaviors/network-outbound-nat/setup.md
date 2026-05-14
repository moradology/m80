# Outbound NAT Bridge And TAP Setup

Behavior capture for bridge/tap setup leaves under `m80-exy.3`.

## Bridge Idempotency

The system treats bridge setup as idempotent when
`<run_root>/outbound-bridge-state.json` already matches the planned bridge
identity and is in the `ready` phase. In that case setup verifies that the
bridge link has the expected gateway IPv4/prefix and returns without creating
the bridge, re-adding the address, or bringing the bridge up again.

If a bridge state file exists but the recorded identity differs from the
planned run-root digest, bridge name, CIDR, or gateway, setup fails closed with
`BridgeOwnershipMismatch` before mutating host links or writing per-VM state.

Before any link mutation, setup also rejects a planned bridge CIDR that
overlaps a non-default host route. The derived bridge interface itself is
allowed so repeated setup can reuse its own host route, but unrelated host
routes produce `HostRouteCollision`.

## Bridge State File

The system writes `outbound-bridge-state.json` directly under the run-root. The
file is written through a temp file plus fsync plus rename, and records:

- `schema_version = 1`
- `setup_phase = planned|ready`
- `run_root`
- `run_root_digest`
- `bridge_name`
- `cidr`
- `gateway_ipv4`

The planned phase is written before bridge link mutation. The ready phase is
written only after bridge creation/address/up succeeds, or is preserved when an
already-ready matching bridge is reused.

If a planned bridge state file exists from a prior interrupted setup, startup
checks whether the corresponding kernel bridge still exists. Missing links are
recreated from the planned state before the ready state is written.

Per-VM setup checks sibling VM state files for the planned guest IPv4 before
taking the allocation lock. Under the lock, setup records a per-IP claim under
`<run_root>/.network-guest-ip-claims/<guest_ipv4>` before writing its own
Planned state. The claim closes the state-file race where two concurrent
launches derive the same guest address without walking every sibling VM
directory while the allocation lock is held: at most one colliding VM state can
remain ready.

## Private Namespace Tap Creation

The system creates each per-VM TAP interface without invoking `ip` or
`/sbin/ip`. For `AllowOutbound`, m80 creates a named VMM network namespace, a
host/vmm veth pair, a private bridge inside that namespace, and the TAP inside
that namespace. TAP creation goes through the Linux TUN/TAP driver after m80
enters the VMM namespace; bridge, veth, attach, isolation, link-up, and cleanup
operations use rtnetlink.

Bridge-port isolation is part of the per-VM isolation boundary. Host-side veth
ports on the same run-root bridge must not be able to exchange L2 traffic
directly through the bridge; routed egress is controlled later by the per-VM
bridge-and-guest-IPv4-scoped FORWARD rules and NAT policy. The VMM-local bridge
is not port-isolated because it must carry traffic between the guest TAP and the
veth peer. m80 assigns deterministic VMM-local bridge and TAP link MACs that are
distinct from the guest MAC so gateway replies addressed to the guest MAC are
forwarded to the Firecracker TAP path rather than consumed by a host-side link
device.

`m80-firecracker` calls this setup through the pinned `m80-net-helper` from
launch phase 6 when `NetworkPolicy::AllowOutbound` resolves to
`VmNetworkMode::OutboundNat`. The realized TAP name and guest MAC become the
Firecracker `NetworkInterfaceConfig` for `eth0`, and the planned namespace path
is passed to the official jailer as `--netns`. Launch failure cleanup,
`StoppedSandbox::delete`, `StoppedSandbox::preserve_for_triage`, and stale
run-root recovery route outbound cleanup through the same helper boundary.

If TAP setup fails after the run-root bridge has been created, setup rolls back
the guest-IP claim and VM network state file, deletes the host veth and
m80-owned namespace if they were partially created, and scavenges the now-unused
run-root bridge state. A failed VM launch must not leave a bridge, veth,
namespace, or per-VM state orphan behind.

This is a deliberate correction from the inherited predecessor `ip tuntap` path.
Kata's runtime validates the no-shellout posture for host link management, but
Linux accepts TUN/TAP creation through `/dev/net/tun`, not as an rtnetlink
`RTM_NEWLINK` create operation. The m80 split is therefore:

- TAP creation: Linux TUN/TAP driver inside the VMM namespace.
- Bridge/address/veth/link mutation, namespace link moves, and deletion:
  rtnetlink.
- Firewall policy: iptables/sysctl policy work, applied through
  `m80-net-helper` after PID-1 guest network tokens are prepared.

## No IP Shellout

Bridge/tap setup has no `ip link`, `ip addr`, `ip tuntap`, `ip link delete`,
or `/sbin/ip` command path. Privilege is held by the network helper process
after startup and consumed through direct kernel APIs: rtnetlink for link
mutation/deletion
and the TUN/TAP driver for TAP creation.

The no-shellout contract applies to bridge/tap setup only. iptables policy and
sysctl configuration remain separate network-policy work.

## VM State File

The system writes `<run_dir>/network-state.json` through the same atomic
temp-file plus fsync plus rename pattern. The file records:

- `schema_version = 1`
- `setup_phase = planned|ready`
- `vm_id`
- `run_dir`
- embedded bridge state
- `iface_id = eth0`
- `tap_name`
- `guest_mac`
- `guest_ipv4`
- `private_ipv4_exceptions`
- `dns_resolvers`
- `runtime_rootfs_configured`

The planned phase is written before per-VM TAP mutation. The ready phase is
written only after TAP creation, MAC assignment, bridge attach, and link-up
succeed.

The guest-IP claim is hidden run-root state owned by the same VM id. Normal
cleanup removes the claim before removing the VM network state. If cleanup is
called after the state file is already gone, it derives the guest IP from
`(run_root, vm_id)` and removes that claim opportunistically.

## Verification

`crates/m80-net-outbound/tests/network-outbound-nat/setup.rs` pins the
bridge idempotency, atomic bridge state file, atomic per-VM state file, TAP
bridge-port isolation, and source-level no-`ip` contract for the link-ops
implementation. The crate also keeps unit coverage in `src/link_ops.rs` for
TAP/bridge lifecycle ordering and an ignored root/CAP_NET_ADMIN probe that
creates and deletes a real TAP through the no-`/sbin/ip` path.

Relevant setup tests:
`bridge_setup_is_idempotent_with_matching_state`,
`planned_bridge_state_recovers_existing_kernel_bridge_without_recreate`, and
`planned_bridge_state_recreates_kernel_dropped_bridge`. Failed setup cleanup is
pinned by `failed_launch_after_bridge_cleans_bridge`. Guest-IP claim ownership
is pinned by `vm_network_setup_writes_guest_ip_claim` and
`cleanup_removes_guest_ip_claim`. Concurrent guest-IP collision handling is
pinned by `concurrent_launch_no_ipv4_collision`.
Host-route pre-mutation rejection is pinned by
`host_route_collision_returns_typed_error_pre_mutation`.
