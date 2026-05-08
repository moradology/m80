# Outbound NAT Teardown

## Rule Comment Prefix

Every iptables rule owned by m80 carries
`--comment "m80:<first 12 hex chars of sha256(run_root)>:<tap_name>"`.
This applies to per-VM filter-chain entries, FORWARD entries, and NAT
POSTROUTING masquerade. Cleanup identifies owned rules by this exact comment,
not by list position.

Source: predecessor `LEGACY_RULE_COMMENT_PREFIX` line 30, renamed for m80, and
`outbound_nat_rule_comment` lines 1503-1511.

Verification:
`crates/m80-net-outbound/tests/network-outbound-nat/teardown.rs::every_owned_rule_carries_per_vm_comment`.

## Cleanup By Comment

During policy teardown, m80 lists the owned filter chain with
`iptables -w -t filter -S <chain>`. Rules in that chain must contain the
per-VM comment before they are deleted. A rule in the owned chain without the
comment aborts cleanup with `ForeignChainRule`; unrelated rules outside the
owned chain are left alone.

Source: predecessor `cleanup_outbound_nat_policy` lines 1202-1212 and
`delete_owned_iptables_chain_rules` lines 1819-1840.

Verification:
`crates/m80-net-outbound/tests/network-outbound-nat/teardown.rs::cleanup_deletes_only_rules_with_owned_comment`
and `::cleanup_foreign_rule_in_owned_chain_aborts`.

## Host Forwarding Sysctl

Policy setup enables IPv4 forwarding with `sysctl -w net.ipv4.ip_forward=1`
before installing FORWARD or NAT rules. Teardown does not revert that host-wide
sysctl. m80 owns the per-VM rules it stamped with the m80 comment; it does not
try to infer whether some other host workload still needs forwarding.

Verification:
`crates/m80-net-outbound/tests/network-outbound-nat/iptables.rs::sysctl_ip_forward_set_before_rules`
and `crates/m80-net-outbound/tests/network-outbound-nat/teardown.rs::cleanup_does_not_revert_host_ip_forward_sysctl`.

## NAT Masquerade Lifecycle

Setup appends one owned `nat/POSTROUTING` MASQUERADE rule for the guest `/32`.
Teardown deletes that exact rule. After deleting it, cleanup lists
`nat/POSTROUTING`; any residual NAT rule carrying the same per-VM m80 comment is
treated as owned-rule drift and fails closed instead of being silently left
behind.

Unrelated host NAT rules without the per-VM m80 comment are not owned by m80
and are left alone.

Verification:
`crates/m80-net-outbound/tests/network-outbound-nat/iptables.rs::nat_postrouting_masquerade_for_guest_source`,
`crates/m80-net-outbound/tests/network-outbound-nat/teardown.rs::cleanup_removes_nat_masquerade_rule`, and
`crates/m80-net-outbound/tests/network-outbound-nat/teardown.rs::residual_nat_rule_with_owned_comment_blocks_cleanup`.

## Chain Delete

After deleting all comment-owned rules, m80 lists the per-VM filter chain
again. It deletes the chain with `iptables -w -t filter -X <chain>` only when
the chain is empty. Any residue produces a typed conflict instead of deleting
through ambiguity.

Source: predecessor `delete_iptables_chain_if_empty` lines 1842-1880.

Verification:
`crates/m80-net-outbound/tests/network-outbound-nat/teardown.rs::chain_deleted_only_when_empty`.

## Tap Delete Tolerant

VM cleanup deletes the TAP through the link-operation seam's
`delete_link_if_exists`. The operation is idempotent: repeated cleanup calls
after the VM network state file is removed do not surface a missing-device
error and do not attempt a second TAP deletion.

When the per-VM network state file is missing, cleanup still derives the TAP
name from `(run_root, vm_id)` and deletes that link before running orphan bridge
scavenging. This catches interrupted setups where a TAP survived but
`network-state.json` did not.

Source: predecessor `delete_interface_if_present` lines 1301-1318 and
`cleanup_vm_network_with_host` lines 1101-1121. m80 uses rtnetlink through
`LinkOps` instead of shelling out to `ip`.

Verification:
`crates/m80-net-outbound/tests/network-outbound-nat/teardown.rs::repeated_cleanup_calls_are_safe`
and `crates/m80-net-outbound/tests/network-outbound-nat/ownership.rs::orphan_tap_detected_and_cleaned`.

`m80-firecracker` arms an outbound-network cleanup guard after successful
phase-6 realization. If any later launch phase fails, the guard calls
`cleanup_vm`. A successful launch carries the cleanup obligation in the
`RunningSandbox` and then into `StoppedSandbox`; `delete` and
`preserve_for_triage` reap owned network residue before releasing the VM
lifetime.

## Foreign Rule Rejection

Before installing into a pre-existing per-VM filter chain, m80 lists the
chain and fails closed when any rule beyond the chain header lacks the
expected per-VM comment. This prevents accidental coexistence with foreign
host firewall policy in a chain name m80 would otherwise own.

Source: predecessor `reject_foreign_iptables_chain_rules` lines 1782-1817.

Verification:
`crates/m80-net-outbound/tests/network-outbound-nat/teardown.rs::foreign_rule_in_owned_chain_aborts`.
