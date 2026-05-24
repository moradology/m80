# Snapshot Restore Edge Behaviors

Behaviors captured by `m80-5vlb.2`.

## Corrupted Snapshot Files

A captured snapshot pair is not trusted just because the files exist. If either
snapshot file is truncated or otherwise changed after capture,
`m80-snapshot::restore` rejects it by recomputing the manifest sha256s before
Firecracker sees `PUT /snapshot/load`. If Firecracker still rejects a snapshot
after the manifest check, m80 surfaces a finite typed restore error instead of
panicking or treating the VM as usable.

Tests:
`crates/m80-snapshot/tests/restore.rs::restore_rejects_tampered_memory_before_load`;
`crates/m80-firecracker/tests/snapshot_integration.rs::corrupted_snapshot_file_fails_clearly`
(#[ignore]) captures a real VM, truncates the memory snapshot file, attempts a
restore, and asserts the error display is classified as snapshot, client, or
I/O restore failure.

## Firecracker Version Pin

The snapshot manifest records the capture-time Firecracker version pin. Restore
compares that value with the restore environment's expected Firecracker version
and fails before vsock unlink or REST calls when they differ. Snapshot restore
does not rely on Firecracker's own compatibility warning/error behavior.

Test: `crates/m80-snapshot/tests/restore.rs::restore_rejects_firecracker_version_mismatch_before_load`.

## Live Firecracker Version Validation

After manifest validation, restore asks the target Firecracker process for
`GET /version` and compares that live value with the snapshot's expected
Firecracker version. Firecracker's API reports the raw Cargo version
(`1.15.1`), so m80 converts it to the existing `v`-prefixed pin form before
comparison. A mismatch fails as `SnapshotError::VersionMismatch` before stale
vsock unlink and before `PUT /snapshot/load`.

Tests:
`crates/m80-snapshot/tests/version_validation.rs::version_mismatch_is_rejected_before_load`;
`crates/m80-snapshot/tests/version_validation.rs::version_match_proceeds_to_load`.

## Post-Capture Mutation

Snapshot files are source artifacts for later restores. Mutating a VM restored
from a snapshot must not rewrite the source snapshot pair in a way that changes
future restores from the same pair.

Test: `crates/m80-firecracker/tests/snapshot_integration.rs::post_capture_mutation_does_not_change_snapshot_restore_state`
(#[ignore]) captures a VM with `/tmp/snapshot-value=before`, restores it once
and writes `after`, deletes that VM, then restores from the original snapshot
again and asserts the value is still `before`.

## Interrupted Restore Recovery

If the host process dies during snapshot restore, startup recovery treats the
partial run directory the same way as other stale VM residue: a stale
`ownership.lock` with no live owner lets recovery remove the partial run dir,
including snapshot bind-target directories and scratch files under it.

Test: `crates/m80-firecracker/tests/snapshot_integration.rs::interrupted_snapshot_restore_run_dir_recovery_removes_partial_state`
constructs a partial restore run dir with a stale owner marker and snapshot
subdirectory, then asserts `Backend::recover_stale_run_root()` removes it.
