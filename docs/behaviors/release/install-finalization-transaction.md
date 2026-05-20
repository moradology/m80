# Install Finalization Transaction

Behavior bead: `m80-o3uh9.16.1`.

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

If bundle verification, host-binaries manifest generation, profile writing, or
the interruption injection fails, any previous `<install-root>/active` symlink
continues to select the previous version. An unselected version directory may
remain after a late failure so the operator can inspect it; it is not current
until the active symlink points at it.

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
- `install_bundle_layout_cleans_abandoned_staging_dirs`
- `install_finalization_transaction_doc_names_state_machine_and_tests`
