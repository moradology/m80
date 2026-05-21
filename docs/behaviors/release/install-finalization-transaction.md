# Install Finalization Transaction

Behavior beads: `m80-o3uh9.16.1`, `m80-o3uh9.16.8.3`,
`m80-o3uh9.16.11`.

`m80 install --bundle-url <URL>` publishes a version only after the installer
has enough local state to run it. The active install selection is the last write
in the transaction.

## Finalization Order

The installer records this order in command output:

1. `bundle_verification`
2. `host_prerequisite_verification`
3. `install_provenance`
4. `host_binaries_manifest`
5. `default_profile`
6. `preflight_smoke_gate`
7. `active_pointer_flip`

Official release installs insert `release_proof_cache` after
`install_provenance` and before version-directory publication. That step copies
the verified public integrity material into the staged version directory,
rehashes the proof-cache manifest, and checks saved proof file modes before any
profile/config write or active-pointer rename can happen.

Same-version official reinstalls compare the newly verified bundle and public
proof material against the active installed version before active-state writes.
The no-op path requires all of these to verify: raw `<install-root>/active`
target, installed release bytes and file modes, installed `bundle.json`,
installed `SHA256SUMS`, install provenance, generated default profile,
install-owned selector fields in `config.toml`, host-binaries manifest paths
and installed m80 binary hashes, and saved release proof cache. Matching state
returns `state=already_installed`, `files_copied=0`,
`reinstall_status=idempotent_same_material`, and
`next_command=m80 run -- echo hello` without rewriting release bytes, profile,
config, or active pointer. Changed public proof material is a
`proof-cache.reinstall` failure that reports old/new manifest digests and the
changed fields plus the protected `version_dir`, then names
`explicit_repair=review_changed_public_material_then_remove_version_dir_and_reinstall`
and `repair_command=rm -rf -- '<version-dir>' && m80 install --bundle-url ...`
instead of silently replacing the cache. Changed installed bytes, stale profile
state, stale config, stale host manifest, or stale active pointer fail closed
with the same exact `repair_command` and do not overwrite by default.

CI fixtures can opt into a debug-only hostless preflight fixture mode for local
`file://` and local HTTP fixture bundles. GitHub release bundle URLs use live
host preflight. Local bundles without the fixture switch also use live host
preflight before activation. The host-binaries manifest is generated from the
final installed paths: `bin/m80`, `bin/m80-jailer-harden`,
`bin/m80-net-helper`, the configured Firecracker binary, the configured jailer
binary, and the configured Firecracker seccomp filter.

## Active Pointer Rule

The installer writes `<install-root>/active` as an absolute symlink to
`<install-root>/versions/<release_tag>`. That symlink is created through a
temporary sibling and renamed into place after profile writing, manifest
generation, and the preflight gate have passed.

If bundle verification, release proof-cache verification, host-binaries manifest
generation, profile writing, or the interruption injection fails, any previous
`<install-root>/active` symlink continues to select the previous version. An
unselected version directory may remain after a late failure so the operator can
inspect it; it is not current until the active symlink points at it.
Proof-cache write, manifest-digest, and mode failures also leave the attempted
version unpublished, do not create runtime state, and do not leak active
`layout-*` staging directories.

## Staging Cleanup

Each install uses `<install-root>/.staging/layout-<pid>`. Before starting, the
installer removes abandoned `layout-*` staging directories. The active staging
directory is removed on success and on ordinary error returns.

## Tests

- `install_bundle_layout_copies_verified_bundle_into_version_dir`
- `install_bundle_layout_missing_required_bundle_file_fails_before_activation`
- `install_bundle_layout_manifest_failure_leaves_previous_active_selected`
- `install_bundle_layout_profile_failure_leaves_previous_active_and_profile`
- `install_bundle_layout_injected_interruption_leaves_previous_active_selected`
- `proof_cache_write_failure_leaves_previous_active_profile_and_config_selected`
- `proof_cache_manifest_digest_failure_leaves_previous_active_profile_and_config_selected`
- `proof_cache_mode_failure_leaves_previous_active_profile_and_config_selected`
- `write_verified_release_proof_cache_rejects_existing_cache_target_file`
- `install_bundle_layout_cleans_abandoned_staging_dirs`
- `same_version_reinstall_with_identical_proof_material_is_idempotent`
- `same_version_reinstall_with_stale_installed_byte_refuses_explicit_repair`
- `same_version_reinstall_with_missing_installed_byte_refuses_explicit_repair`
- `same_version_reinstall_with_missing_proof_cache_manifest_refuses_explicit_repair`
- `same_version_reinstall_with_stale_host_manifest_refuses_explicit_repair`
- `same_version_reinstall_with_stale_profile_kernel_kind_refuses_explicit_repair`
- `same_version_reinstall_with_missing_default_profile_refuses_explicit_repair`
- `same_version_reinstall_with_missing_installed_config_refuses_explicit_repair`
- `same_version_reinstall_refuses_when_existing_version_is_not_active`
- `same_version_reinstall_refuses_when_active_pointer_is_missing`
- `same_version_reinstall_change_error_names_explicit_repair_version_dir`
- `install_finalization_transaction_doc_names_state_machine_and_tests`
