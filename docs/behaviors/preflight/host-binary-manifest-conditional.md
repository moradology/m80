# Host Binary Manifest Conditional Wrapper

`host-binaries.manifest.json` schema v5 makes `m80_jailer_harden` conditional
instead of universally required. The release bundle may still carry the wrapper
for no-systemd hosts, but the flat installed client path on systemd hosts may
omit `/opt/m80/bin/m80-jailer-harden`.

The only accepted conditional row today is:

```json
{
  "name": "m80_jailer_harden",
  "absent_when": "systemd_path_chosen"
}
```

Preflight behavior is fail-closed:

- wrapper launch selected: the manifest must contain a concrete
  `m80_jailer_harden` row and the configured wrapper binary must exist;
- systemd launch selected and concrete wrapper row present: the row is verified
  like every other host binary;
- systemd launch selected and wrapper row absent: preflight passes only when the
  conditional row above is present;
- systemd launch selected and wrapper row absent without the conditional row:
  preflight fails with `HostBinaryMissing`.

Evidence:

- `crates/m80-preflight/src/binary.rs::tests::missing_jailer_hardening_wrapper_is_allowed_for_systemd_discovery`
- `crates/m80-preflight/src/binary.rs::tests::host_binaries_manifest_generator_can_omit_systemd_wrapper`
- `crates/m80-preflight/src/binary.rs::tests::host_binary_manifest_allows_absent_wrapper_only_for_systemd_path`
- `crates/m80-preflight/src/binary.rs::tests::host_binary_manifest_without_conditional_wrapper_fails_for_systemd_path`
- `crates/m80-cli/tests/release/installer_layout.rs::install_bundle_layout_omits_flat_wrapper_when_systemd_launch_is_selected`
