# Storage Admissibility And Swap

## admissibility

Scratch hydration and post-stop extraction admit only directories and regular
files. Symlinks are refused as `RejectionReason::Symlink`; fifos, sockets,
block devices, character devices, and any other non-regular entry type are
refused as `RejectionReason::SpecialFile`.

Hydration fails the operation with `StorageError::AdmissibilityRefused { path }`.
Extraction records rejected entries in `ChangeSet::rejected` and does not copy
them into the staged tree.

Regular-file hydration opens each source with `O_NOFOLLOW` at the copy point.
If a workspace entry is swapped from regular file to symlink after metadata
inspection, the open fails and no symlink target content is copied into the
scratch image.

Test: `crates/m80-storage/tests/scratch_admissibility.rs`.
Test: `crates/m80-storage/src/scratch.rs::tests::copy_regular_file_no_follow_rejects_symlink_at_open`.

## atomic-swap

Extraction stages the surviving tree in a sibling directory under the
destination parent, unmounts the scratch image, and then renames the staged tree
into the destination. Existing destinations and rename failures surface as
`StorageError::SwapFailed`.

The storage crate does not merge, patch, or silently recover a destination. The
caller chooses whether to retry, preserve the scratch image, or delete the
sandbox.

Test: `crates/m80-storage/tests/storage/change_extraction.rs::rollback_on_extract_failure`.
