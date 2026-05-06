# Jailer Scavenge

## recover-from-run-dir

`recover_from_run_dir` inspects `<run_dir>/jailer-state.json`. If no state file
exists, the run-dir has no jailer residue from this crate's point of view. If
state exists, recovery loads `<run_dir>/jailer-plan.json` when available so an
orphaned jail can be reaped in reverse plan order.

Source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/jailer.rs`
`recover_jailer_from_run_dir` lines 452-455 and
`recover_jailer_from_run_dir_with_ops` lines 722-727.

Test: `crates/m80-jailer/tests/recover.rs::no_state_file_returns_no_jail`.
Test: `crates/m80-jailer/tests/recover.rs::stale_state_with_nonexistent_pids_returns_orphan`.

## identify-orphans

When both persisted pids are present and both `/proc/<pid>` entries exist,
recovery returns `RecoveryDecision::LiveJail`. Otherwise it returns
`RecoveryDecision::OrphanJail` with the known reap steps, leaving the caller to
decide whether and when to kill or remove residue.

Source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/jailer.rs`
`discover_running_jailer_state` lines 457-464 and child-resolution lines
1285-1363.

Test: `crates/m80-jailer/tests/recover.rs::live_state_with_own_pid_returns_live_jail`.

## preserve-residue

Ambiguous jailer state is non-destructive at the crate boundary.
`m80-jailer` reports `OrphanJail`; the orchestrator decides how to handle the
residue during run-root recovery. That keeps uncertainty visible instead of
silently deleting a maybe-live jail.

Source: dossier `07-modules-essential-vs-hygiene.md` jailer startup scavenging
notes and predecessor `crates/sandbox/agent-sandbox-firecracker/src/jailer.rs`
lines 1347-1363.

Test: `crates/m80-jailer/tests/recover.rs::mixed_live_and_dead_pid_returns_orphan`.
