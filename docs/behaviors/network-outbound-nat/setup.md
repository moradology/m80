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

## Tap Creation

The system creates each per-VM TAP interface without invoking `ip` or
`/sbin/ip`. TAP creation goes through the Linux TUN/TAP driver, then the
resulting interface is managed through rtnetlink: set the guest MAC, attach
the TAP to the run-root bridge, and bring the TAP link up.

If TAP setup fails after the run-root bridge has been created, setup rolls back
the VM network state file, deletes the TAP if it was partially created, and
scavenges the now-unused run-root bridge state. A failed VM launch must not
leave a bridge or per-VM state orphan behind.

This is a deliberate correction from the inherited predecessor `ip tuntap` path.
Kata's runtime validates the no-shellout posture for host link management, but
Linux accepts TUN/TAP creation through `/dev/net/tun`, not as an rtnetlink
`RTM_NEWLINK` create operation. The m80 split is therefore:

- TAP creation: Linux TUN/TAP driver.
- Bridge/address/link mutation and deletion: rtnetlink.
- Firewall policy: iptables/sysctl policy work, outside this setup leaf.

## No IP Shellout

Bridge/tap setup has no `ip link`, `ip addr`, `ip tuntap`, `ip link delete`,
or `/sbin/ip` command path. Privilege is held by the m80 process at startup
and consumed through direct kernel APIs: rtnetlink for link mutation/deletion
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

## Verification

`crates/m80-net-outbound/tests/network-outbound-nat/setup.rs` pins the
bridge idempotency, atomic bridge state file, atomic per-VM state file, and
source-level no-`ip` contract for the link-ops implementation. The crate also
keeps unit coverage in `src/link_ops.rs` for TAP/bridge lifecycle ordering and
an ignored root/CAP_NET_ADMIN probe that creates and deletes a real TAP through
the no-`/sbin/ip` path.

Relevant setup tests:
`bridge_setup_is_idempotent_with_matching_state`,
`planned_bridge_state_recovers_existing_kernel_bridge_without_recreate`, and
`planned_bridge_state_recreates_kernel_dropped_bridge`. Failed setup cleanup is
pinned by `failed_launch_after_bridge_cleans_bridge`.
