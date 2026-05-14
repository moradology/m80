# Warm Pool Snapshot Immutability

Behavior capture for bead `m80-8emae.31`.

## Contract

The foreground warm owner captures a golden snapshot pair, then marks
`vm.snap`, `mem.snap`, and `snapshot-manifest.json` read-only before using that
pair to fill the warm pool. The manifest is locked with the snapshot files
because it is the integrity baseline for later slot fills.

`WarmPool` verifies the snapshot manifest before each slot restore attempt. A
tampered snapshot pair fails before the pool admits and launches a replacement
slot. `Sandbox::launch_from_snapshot` still verifies again inside the restore
path before Firecracker receives `PUT /snapshot/load`.

## Non-Contract

The read-only mode is a local filesystem hardening step, not a complete
same-uid write barrier. Stronger immutable-attribute or privilege-separated
snapshot serving remains a separate hardening option.

## Verification

- `crates/m80-cli/src/cmds/warm/owner.rs::tests::lock_warm_snapshot_files_marks_pair_and_manifest_readonly`
- `crates/m80-firecracker/src/warm_pool.rs::tests::warm_snapshot_verify_accepts_matching_manifest`
- `crates/m80-firecracker/src/warm_pool.rs::tests::warm_snapshot_verify_rejects_tampered_memory_before_slot_launch`
- `crates/m80-firecracker/src/warm_pool.rs::tests::warm_pool_fill_rejects_tampered_snapshot_before_admission`
- `crates/m80-snapshot/tests/restore.rs::restore_rejects_tampered_memory_before_load`
