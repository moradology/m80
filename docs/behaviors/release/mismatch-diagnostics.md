Release mismatch diagnostics keep first-run failures actionable. A user with
only the `m80` binary should not have to infer the missing release bundle from
`kernel image not found`, `rootfs image not found`, or an unrelated host
preflight failure.

## No Installed Profile

`m80 run -- echo hello` fails before backend or host-preflight work when the
selected runtime profile is the built-in `env` profile and none of
`M80_KERNEL_IMAGE`, `M80_ROOTFS_IMAGE`, or `M80_ARTIFACT_DIR` is set. Human
stderr names `no installed default profile`; JSON stderr uses the shared error
envelope with `variant: "Config"` and `code: "no_installed_profile"`.

Release builds print both repair forms:

```text
current version: curl -fsSL https://github.com/moradology/m80/releases/download/<tag>/install.sh | sudo sh
latest stable: curl -fsSL https://github.com/moradology/m80/releases/latest/download/install.sh | sudo sh
```

Dev builds must not suggest mutable public latest. They print the explicit
local/operator path instead:

```text
dev build: create a local bundle for <version>-dev and run m80 install --bundle-url file:///path/to/m80-linux-x86_64.tar.gz
```

Explicit artifact input remains authoritative. If the caller sets
`M80_KERNEL_IMAGE` and `M80_ROOTFS_IMAGE`, or sets `M80_ARTIFACT_DIR`, `m80 run`
continues to normal profile-aware preflight.

## Stale Installed Profile

An install-owned default profile with missing referenced paths fails before
unrelated host checks. The diagnostic names the missing profile fields and the
same install repair command for the running binary identity.

## Manifest Schema Mismatch

A stale installed guest manifest fails as a release mismatch, not as a generic
preflight failure. The diagnostic fields are:

- `expected_schema`: the schema this m80 build accepts.
- `actual_schema`: the schema read from the guest manifest.
- `manifest_path`: the installed manifest file that was read.
- `running_m80_version`: the host binary version rendering the error.
- `selected_install_profile`: the runtime/install profile that selected the
  stale artifacts.
- `repair`: the exact reinstall command for the running release, or the local
  bundle command for a dev build.

Install-time bundle validation uses the same field names and additionally names
the bundle `release_tag`.

## Guest Protocol Mismatch

A bundle whose host protocol, guest protocol, or guestd/rootfs identity does not
match the running m80 binary fails before activation. The diagnostic fields are:

- `expected_protocol`: `m80_proto::PROTOCOL_VERSION` for the running binary.
- `actual_m80_protocol`: the protocol recorded for the host side in
  `bundle.json`.
- `actual_guest_protocol`: the protocol recorded for the guest side in
  `bundle.json`.
- `guestd_identity`: guestd package version and bundle sha256.
- `rootfs_identity`: image kind and rootfs bundle sha256.
- `running_m80_version`: the host binary version rendering the error.
- `release_tag`: the selected release tag.
- `repair`: the exact pinned reinstall command for that release tag.

## Binary Vs Bundle Mismatch

Public release selectors fail closed when the running binary and selected
bundle do not agree. Text and JSON diagnostics name:

- `binary_version`: the running m80 package version.
- `binary_release_tag`: the release tag compiled into the running m80 binary,
  when present.
- `bundle_version`: the selected bundle or release tag.
- `release_tag`: the selected release tag.
- `repair`: the exact pinned `install.sh` command for the selected release.

Host-prerequisite failures use the shared repair token catalog in
`docs/behaviors/release/host-prerequisite-policy.md`. `m80 preflight`, `m80
env`, install preflight failures, and JSON error envelopes all render the same
`HostPrerequisiteCheck.remediation.id` for the same typed failure.

Regression coverage:

- `crates/m80-cli/src/cmds/tests.rs::run_default_env_profile_without_artifacts_prints_install_repair`
- `crates/m80-cli/src/cmds/tests.rs::run_default_env_profile_with_artifact_env_is_allowed_to_preflight`
- `crates/m80-cli/src/cmds/tests.rs::stale_manifest_schema_diagnostic_names_profile_path_versions_and_repair`
- `crates/m80-cli/src/cmds/tests.rs::release_no_profile_repair_names_current_and_latest_installer`
- `crates/m80-cli/src/cmds/tests.rs::stale_installed_profile_paths_print_reinstall_command`
- `crates/m80-cli/src/cmds/install/layout/metadata.rs::tests::protocol_mismatch_names_expected_actual_guest_rootfs_and_repair`
- `crates/m80-cli/src/cmds/install/layout/metadata.rs::tests::manifest_schema_mismatch_names_path_profile_versions_and_repair`
- `crates/m80-cli/src/cmds/install/tests.rs::release_tag_source_rejects_binary_tag_mismatch`
- `crates/m80-cli/src/cmds/install/tests.rs::bundle_url_rejects_release_binary_tag_mismatch`
- `crates/m80-cli/src/cmds/quickstart/release_url.rs::tests::mismatch_reason_points_to_pinned_installer`
- `crates/m80-cli/tests/output_error_contract.rs::run_without_installed_profile_reports_repair_code_before_preflight`
