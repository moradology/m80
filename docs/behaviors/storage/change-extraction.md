# Storage Change Extraction

## opt-in

Post-stop change extraction is caller-driven. `m80-storage` never writes back a
workspace merely because a guest process exits, and it does not inspect agent
concepts such as read-only versus mutating effects. The caller either invokes
`Scratch::extract(image, into, max_extract_bytes)` or discards/preserves the
scratch image through the surrounding lifecycle.

Source: dossier `01-coupling-audit.md` writeback model notes and
`07-modules-essential-vs-hygiene.md` storage module notes.

Test: `crates/m80-storage/tests/storage/change_extraction.rs::extraction_only_when_requested`.

## debugfs-rdump

predecessor extracted the scratch image with `debugfs -R "rdump / <extract_root>"`.
m80 v0.1 deliberately uses `e2fsck -p -f` followed by a read-only loop mount
instead. The observable contract remains: the stopped scratch filesystem is
read, regular files and directories are copied into a staging tree, and
inadmissible entries are reported or refused by the storage layer. The debugfs
parser surface is not part of m80 v0.1.

Source: predecessor
`crates/sandbox/agent-sandbox-firecracker/src/storage.rs`
`extract_workspace_image` lines 186-198; m80 `crates/m80-storage/README.md`
section "v0.1 departure: loop-mount instead of debugfs".

Test: `crates/m80-storage/tests/storage/change_extraction.rs::loop_mount_extracts_changed_file_set`.

## size-cap

Extraction accepts an optional maximum byte count for staged regular files. The
walk checks the cap before copying each regular file and fails with
`StorageError::ExtractSizeExceeded` when admitting that file would push
`ChangeSet::total_bytes` beyond the cap. The surrounding lifecycle passes the
scratch image length as the default cap for `StoppedSandbox::extract_changes`,
so a guest cannot force host extraction of more regular-file bytes than the
scratch device was sized to hold.

Test: `crates/m80-storage/src/scratch.rs::tests::build_stage_rejects_extract_size_over_cap`.

## mode-high-bits

Extraction strips setuid, setgid, and sticky mode bits from staged regular
files and directories. Low host-visible permission bits are preserved, so an
extracted `04755` file is published as `0755`.

Tests:

- `crates/m80-storage/src/scratch.rs::tests::build_stage_strips_setuid_bits_from_extracted_file`
- `crates/m80-storage/src/scratch.rs::tests::build_stage_strips_setgid_bits_from_extracted_dir`

## staging

Extraction materializes survivors into a sibling staging directory before the
destination workspace is swapped. The staging directory is created under the
destination parent with the prefix:

```text
.<workspace>.m80-writeback-stage-<pid>-
```

Keeping the stage beside the destination preserves the same-filesystem rename
invariant and keeps an in-progress extraction out of the live workspace.

Source: predecessor
`crates/sandbox/agent-sandbox-firecracker/src/storage.rs`
`create_workspace_stage_dir` lines 384-411 and `sanitize_workspace_tree` lines
413-475.

Test: `crates/m80-storage/src/scratch.rs::tests::stage_prefix_names_sibling_writeback_stage`.
Fixture: `crates/m80-storage/tests/storage/change_extraction.rs::stages_into_sibling_directory`.

## rollback

Extraction failure leaves the destination workspace unchanged. If the
destination already exists, `Scratch::extract` fails with
`StorageError::SwapFailed` before checking or mounting the image. If staging
fails, the temporary staging directory is dropped. If the final rename fails,
the failure is surfaced as `SwapFailed` and the caller's destination remains
the authority.

Source: predecessor
`crates/sandbox/agent-sandbox-firecracker/src/storage.rs`
`write_back_workspace_with_stage_renamer` lines 71-109 and
`cleanup_stage_root` lines 570-578.

Test: `crates/m80-storage/tests/storage/change_extraction.rs::rollback_on_extract_failure`.
