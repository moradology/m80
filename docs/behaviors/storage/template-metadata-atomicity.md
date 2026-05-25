# Overlay Template Metadata Atomicity

The rootfs overlay template is published as a pair:

- `.rootfs-overlay-template-v<schema>-<size>.ext4`
- `.rootfs-overlay-template-v<schema>-<size>.meta`

The ext4 template is promoted from a same-directory temp file with `rename`.
The metadata sidecar follows the same rule: `m80-storage` writes the expected
metadata bytes to `.meta.<pid>.tmp`, fsyncs that file, and renames it over the
final `.meta` path. A partial metadata write is therefore never the published
template identity.

Existing templates still fail closed when their metadata is missing or does not
match the requested template size and schema. m80 does not infer a replacement
identity from the ext4 image alone.

Tests:

- `crates/m80-storage/src/rootfs.rs::tests::template_metadata_publish_replaces_tmp_with_final_sidecar`
- `crates/m80-storage/tests/rootfs_prepare.rs::stale_template_metadata_is_a_hard_error`
