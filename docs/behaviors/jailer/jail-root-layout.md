# Jailer Root Layout

## per-vm-jail-root

Each jailed VM has one Firecracker jailer root derived from the VM run
directory. m80 passes the per-VM `run_dir` as jailer's `--chroot-base-dir` and
the run directory basename as jailer's `--id`; Firecracker's jailer then
materializes:

```text
<run_dir>/<firecracker basename>/<run_dir basename>/root
```

This is a hard cutover from predecessor's older `<run_dir>/jail-root` helper name.
m80 follows the official jailer's hardcoded nested layout instead of inventing a
second leaf.

Source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/foundation.rs`
`JAIL_ROOT_DIR` line 44 and predecessor
`crates/sandbox/agent-sandbox-firecracker/src/jailer.rs` lines 767, 783-786.

Test: `crates/m80-jailer/tests/jailer/jail_root_layout.rs::jail_root_under_run_dir`.

## chroot-base

The jailer chroot base is caller-configured through `JailerConfig::run_dir`.
`m80-jailer` does not probe a global `/var/tmp/...` base and does not silently
fall back to a different system path. The orchestrator owns run-root selection
and preflight; this crate computes and materializes the jail under the supplied
per-VM run directory.

Source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/jailer.rs`
`OFFICIAL_JAILER_BASE_ROOT` line 36 and `projected_jailer_chroot_base_dir_with`
lines 470-504. m80 intentionally replaces that probe/fallback branch with a
single caller-supplied run-dir contract.

Test:
`crates/m80-jailer/tests/jailer/jail_root_layout.rs::chroot_base_is_caller_configured_run_dir`.

## plan-persisted

`Plan::materialize` persists the computed plan as
`<run_dir>/jailer-plan.json` after applying the jail steps. The persisted JSON
is replayable for recovery and offline triage.

Source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/foundation.rs`
`JAILER_PLAN_FILE` line 45 and
`crates/sandbox/agent-sandbox-firecracker/src/jailer.rs`
`write_prepared_jailer_plan` lines 341-358.

Test: `crates/m80-jailer/tests/integration_root.rs::materialize_creates_jail_root_and_persists_plan`.

## state-persisted

`Plan::materialize` writes an initial `<run_dir>/jailer-state.json` with no
live pids. `MaterializedJail::launch` updates the same file with
`jailer_pid` and `firecracker_pid` after the jailer has spawned Firecracker.
`recover_from_run_dir` reads that state to classify residue on startup.

Source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/foundation.rs`
`JAILER_STATE_FILE` line 46 and predecessor
`crates/sandbox/agent-sandbox-firecracker/src/jailer.rs`
`JailerRuntimePhase` lines 102-110.

Test: `crates/m80-jailer/tests/recover.rs`.
