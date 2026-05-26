# Storage Edge Coverage

`TemplateLock::acquire` is fail-closed around the shared overlay-template lock.
If the lock file already exists until the wait budget is exhausted, acquisition
returns `StorageError::OverlayTemplateCreateFailed` carrying an
`ErrorKind::TimedOut` source and does not remove the existing lock. If creating
the lock fails for any non-`AlreadyExists` reason, the original I/O kind is
returned through the same typed storage error.

Overlay-template metadata is published through a temporary sidecar followed by
rename. A stale or mismatched final metadata sidecar is a hard error instead of
being overwritten silently during reuse.

Workspace scratch creation rejects symlinks during the real root/loop-backed
image build path, not only in the pure admissibility mirror tests.

This bead also pins the small absorbed manifest edges near the storage layer:
malformed snapshot manifest bytes return `SchemaError::Json`, snapshot
persistence ids reject parent traversal before path joining, Ubuntu image
manifests require both source-rootfs fields, Minimal image manifests require
both source-rootfs fields to be absent, the Ubuntu dry-run plan names the
four-artifact sha256 step, and `KernelKind::Stock` survives manifest
write/read.

## Evidence

- `crates/m80-storage/src/rootfs.rs::tests::template_lock_timeout_returns_timed_out_error_without_removing_lock`
- `crates/m80-storage/src/rootfs.rs::tests::template_lock_open_error_returns_original_io_kind`
- `crates/m80-storage/src/rootfs.rs::tests::template_metadata_publish_replaces_tmp_with_final_sidecar`
- `crates/m80-storage/tests/rootfs_prepare.rs::stale_template_metadata_is_a_hard_error`
- `crates/m80-storage/tests/scratch_create_real.rs::scratch_create_rejects_symlink_in_workspace`
- `crates/m80-snapshot/src/tests/manifest_schema_version.rs::malformed_json_returns_json_error_from_from_bytes`
- `crates/m80-snapshot/tests/persistence_path_validation.rs::persistence_path_rejects_path_traversal_ids`
- `crates/m80-image-manifest/tests/manifest_kind_invariants.rs::ubuntu_with_source_rootfs_sha256_none_is_inconsistent`
- `crates/m80-image-manifest/tests/manifest_kind_invariants.rs::minimal_with_source_rootfs_sha256_set_is_inconsistent`
- `crates/m80-image-build/tests/dry_run_smoke.rs::dry_run_prints_steps_to_stderr_and_creates_no_output_files`
- `crates/m80-image-manifest/tests/schema_v3_kernel_kind.rs::kernel_kind_stock_roundtrip`
