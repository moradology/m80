# Installed Host Binaries Manifest

`host-binaries.manifest.json` is generated after final install paths are known.
It is not a release-bundle payload. The release bundle carries m80-owned
binaries and guest artifacts; the installed host manifest records the concrete
host-side TCB files that preflight must verify before launch.

Schema v4 has two lists:

- `binaries`: executable host TCB files: `firecracker`, `jailer`, `m80`,
  `m80_jailer_harden`, and `m80_net_helper`.
- `launch_material`: non-executable launch-critical files. v4 requires
  `firecracker_seccomp_filter`.

Every entry carries `name`, absolute `path`, `sha256`, and `version`.
Firecracker and jailer versions are the parsed official release strings from
`--version`; m80 helpers report their own `--version`; the seccomp-filter
version is the accepted Firecracker train the filter belongs to.

The Firecracker seccomp filter is deliberately not a `HostBinaryEntry`. It is
opened, hashed, permission-checked, and path-checked like TCB material, but it
is not executable and must not be described as one.

Preflight requires the runtime-configured paths for `firecracker`, `jailer`,
`m80_jailer_harden`, `m80_net_helper`, and `firecracker_seccomp_filter` to
match the manifest. It opens every recorded path with `O_NOFOLLOW`, hashes the
opened file descriptor, and rejects changed bytes. Files must be regular,
owned `root:root`, mode `0755` or narrower, and not group/world writable.
Launch material must also be non-empty. Preflight also compares recorded
versions against the live observed Firecracker/jailer train, m80 helper
`--version` output, and the seccomp filter's owning Firecracker train.

Old v1/v2/v3 host-binaries manifests fail closed with
`ManifestError::UnsupportedHostBinariesSchemaVersion`, which includes the
expected current schema version. Regenerate the manifest from the installed
files after changing any host-side TCB path or bytes.

Evidence:

- `crates/m80-image-manifest/tests/host_binaries_manifest.rs`
- `crates/m80-preflight/src/binary.rs::tests::host_binaries_manifest_generator_records_final_paths_hashes_and_versions`
- `crates/m80-preflight/src/binary.rs::tests::host_binary_manifest_exact_match_succeeds`
- `crates/m80-preflight/src/binary.rs::tests::host_binary_manifest_firecracker_version_mismatch_fails_closed`
- `crates/m80-preflight/src/binary.rs::tests::host_binary_manifest_helper_hash_mismatch_fails_closed`
- `crates/m80-preflight/src/binary.rs::tests::host_binary_manifest_requires_seccomp_launch_material`
- `crates/m80-preflight/src/binary.rs::tests::host_binary_manifest_rejects_duplicate_seccomp_launch_material`
- `crates/m80-preflight/src/binary.rs::tests::host_launch_material_hash_mismatch_fails_closed`
- `crates/m80-preflight/src/binary.rs::tests::host_launch_material_rejects_path_mismatch`
- `crates/m80-preflight/src/binary.rs::tests::host_launch_material_rejects_empty_file`
- `crates/m80-preflight/src/binary.rs::tests::host_launch_material_rejects_unsafe_permissions`
- `crates/m80-preflight/src/binary.rs::tests::host_launch_material_symlink_fails_no_follow_open`
