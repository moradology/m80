# Force-Kill Preservation

## Run-Dir Preserved

`RunningSandbox::force_kill` returns `StoppedSandbox` instead of deleting the
run directory. The caller must make the next decision explicitly:
`StoppedSandbox::delete` removes the run-dir, while
`StoppedSandbox::preserve_for_triage` moves it under
`<run_root>/.preserved/<unix_ms>-<vm_id>/`.

Recovery skips `.preserved/` so offline triage archives are not scavenged as
ordinary orphan run directories.

Tests:
- `crates/m80-firecracker/src/lifecycle/stopped.rs::tests::preserve_for_triage_moves_run_dir_under_preserved`
- `crates/m80-firecracker/tests/cleanup/idempotent_teardown.rs::leaves_unowned_residue_alone`
- `crates/m80-firecracker/tests/cleanup/force_kill_preservation.rs::run_dir_preservation_is_explicit_after_stop`

## Last-Resort Only

Normal `stop` and explicit `force_kill` are separate consuming methods.
Callers choose `force_kill` only when the guest shutdown path is unavailable or
the sandbox state is already suspect. m80 does not hide force kill behind a
placement release decision.

Test:
- `crates/m80-firecracker/tests/cleanup/force_kill_preservation.rs::force_kill_is_separate_last_resort_surface`
