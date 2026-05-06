# Idempotent Teardown

## Repeat Safe

The public lifecycle prevents double-stop by consuming `RunningSandbox`.
Repeated cleanup of stale run-root residue is safe through
`Backend::recover_stale_run_root`: an orphan directory can be reaped once, and a
second scan succeeds with no work left to do.

`StoppedSandbox::delete` also treats an already-missing run directory as clean.
That keeps release idempotent when an external recovery pass removed the
directory before the stopped handle reached delete.

Tests:
- `crates/m80-firecracker/src/lifecycle/stopped.rs::tests::delete_tolerates_already_missing_run_dir`
- `crates/m80-firecracker/tests/cleanup/idempotent_teardown.rs::repeat_calls_succeed`

## Owned Residue Only

`recover_stale_run_root` scans per-VM directories directly under the configured
run-root. It skips live `ownership.lock` holders and skips `.preserved/`, which
is the explicit offline-triage archive. It does not walk outside the configured
run-root.

Outbound NAT cleanup has its own ownership checks in `m80-net-outbound`: it
removes comment-owned iptables rules, the owned TAP, and the owned per-VM state
file while preserving foreign rules and ambiguous bridge state.

Tests:
- `crates/m80-firecracker/tests/cleanup/idempotent_teardown.rs::leaves_unowned_residue_alone`
- `crates/m80-net-outbound/tests/network-outbound-nat/teardown.rs::cleanup_deletes_only_rules_with_owned_comment`
- `crates/m80-net-outbound/tests/network-outbound-nat/teardown.rs::cleanup_foreign_rule_in_owned_chain_aborts`

## Startup Scavenge Reuse

Startup scavenging is explicit: callers invoke
`Backend::recover_stale_run_root`. It uses the same run-dir removal primitive as
orphan recovery after a dead owner: unmount anything below the run-dir, cleanup
the cgroup leaf, then remove the directory. Ambiguous jailer recovery state is
logged and preserved.

Test:
- `crates/m80-firecracker/tests/cleanup/idempotent_teardown.rs::startup_scavenge_uses_same_path`
