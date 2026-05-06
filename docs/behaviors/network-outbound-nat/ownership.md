# Outbound NAT Ownership And Recovery

Behavior capture for bridge ownership and orphan recovery leaves under
`m80-exy.7`.

## Bridge Removal Guard

`cleanup_vm` deletes a run-root bridge only when no peer VM state in that
run-root references the same bridge owner identity. The scan walks sibling
`<run_root>/*/network-state.json` files, excludes the VM currently being
cleaned up, and treats a matching `bridge.run_root` plus `bridge.bridge_name`
as evidence that the shared bridge is still in use.

When a peer reference exists, cleanup removes only the current VM's owned
policy/TAP/state residue and preserves `<run_root>/outbound-bridge-state.json`
and the bridge link.

Source: predecessor
`crates/sandbox/agent-sandbox-firecracker/src/network.rs::cleanup_vm_network_with_host`
lines 1114-1118 and `::other_bridge_users_exist` lines 1320-1351.

Verification:
`crates/m80-net-outbound/tests/network-outbound-nat/ownership.rs::bridge_removed_only_when_no_peer_references`.

## Orphan Recovery

When `cleanup_vm` is called for a VM whose `network-state.json` is missing, it
does not assume that the VM-specific cleanup path can proceed. Instead it runs
the run-root orphan bridge scavenger.

The scavenger removes the bridge only when
`<run_root>/outbound-bridge-state.json` exists and no VM state file remains
under that run-root. It then validates that the bridge owner record's
`run_root` is the cleanup run-root before deleting the bridge link and removing
the owner record.

Source: predecessor
`crates/sandbox/agent-sandbox-firecracker/src/network.rs::cleanup_orphan_bridge_if_unused`
lines 1123-1147 and `::any_vm_network_states_exist` lines 1182-1200.

Verification:
`crates/m80-net-outbound/tests/network-outbound-nat/ownership.rs::startup_scavenges_orphan_bridge_when_unused`.

## Malformed State Tolerance

Peer VM state that exists but cannot be read or parsed is treated as live
residue. Cleanup preserves the bridge in that ambiguous case instead of
guessing that the malformed peer is safe to delete around.

Before deleting a bridge for the last readable VM state, cleanup also compares
the on-disk bridge owner record with the VM state's embedded bridge identity.
An owner mismatch fails closed with `NetworkAllocationConflict`; m80 does not
delete a bridge whose run-root owner record disagrees with the VM state being
cleaned up.

Source: predecessor
`crates/sandbox/agent-sandbox-firecracker/src/network.rs::other_bridge_users_exist`
lines 1341-1343 and `::validate_bridge_owner_record_for_cleanup` lines
1353-1362.

Verification:
`crates/m80-net-outbound/tests/network-outbound-nat/ownership.rs::malformed_peer_state_preserves_bridge`
and
`crates/m80-net-outbound/tests/network-outbound-nat/ownership.rs::bridge_owner_mismatch_blocks_bridge_delete`.

## Crash Mid VM

Crash-mid-VM setup does not poison later startups. Deterministic naming and
owner-record validation make the next startup converge through one of three
paths: reuse a matching Ready bridge, scavenge an orphan bridge when no VM
states remain, or fail closed on ambiguous/mismatched ownership.

This means a leftover bridge owner record without any VM states is cleaned up
before a new VM realizes networking for the same run-root.

Source: predecessor
`crates/sandbox/agent-sandbox-firecracker/src/network.rs::ensure_bridge_ready`
lines 298-303, `::validate_expected_bridge_state` lines 709-724, and
`::cleanup_orphan_bridge_if_unused` lines 1123-1147.

Verification:
`crates/m80-net-outbound/tests/network-outbound-nat/ownership.rs::crash_mid_vm_does_not_break_new_startup`.
