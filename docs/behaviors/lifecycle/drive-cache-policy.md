# Drive Cache Policy

`m80-firecracker-client::DriveConfig` carries an optional typed
`CacheType::{Writeback, Unsafe}` field that serializes to Firecracker's
PascalCase wire values. When unset, `cache_type` is omitted so Firecracker keeps
its default behavior.

Cold-boot preboot planning sets `cache_type = Unsafe` on m80's ephemeral
writable drives: `rootfs_overlay` and optional `workspace`. The shared
read-only `rootfs` drive and preallocated hotplug placeholder drives omit
`cache_type`.

Callers that need Firecracker's conservative host sync behavior set
`SandboxConfig::drive_cache_type = Some(CacheType::Writeback)`. That override
applies only to the writable preboot drives; the read-only base drive remains
unspecified.

Tests:

- `crates/m80-firecracker-client/tests/put_each_resource.rs::cache_type_uses_firecracker_pascal_case`
- `crates/m80-firecracker-client/tests/put_each_resource.rs::put_drive_sends_correct_json_and_url`
- `crates/m80-firecracker/src/preboot_tests.rs::rootfs_overlay_drive_put_after_shared_rootfs`
- `crates/m80-firecracker/src/preboot_tests.rs::scratch_drive_put_with_workspace_id`
- `crates/m80-firecracker/src/preboot_tests.rs::writable_drive_cache_type_override_preserves_writeback`
