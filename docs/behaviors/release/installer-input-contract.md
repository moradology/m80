# Installer Input Contract

Behavior bead: `m80-o3uh9.3.1`.

`m80 install` is the release-bundle installer front door. This behavior pins
the user-facing input contract, dry-run plan, and release-bundle layout copy.
The command does not yet perform privileged copies, write `/etc` profile state,
or resolve "latest". Successful bundle installs write install-root-local
profile state and switch the install-root active pointer.

## Source Selection

The installer accepts exactly one source shape:

- `--release-tag <TAG>`: a pinned release tag selected by the operator.
- `--bundle-url <URL>`: an explicit release bundle URL, including local
  `file://` bundle fixtures used while developing installer behavior.
- `--bootstrap-tag <TAG>`: a concrete tag handed to `m80 install` by the stable
  bootstrapper after it resolves "latest". This flag is hidden from normal help
  because humans should use `--release-tag` or `--bundle-url`.

The parser rejects more than one source. A missing source reaches the command
validator so the diagnostic names the two human-facing choices without exposing
the hidden bootstrapper handoff. Source tags are exact GitHub release tags such
as `v0.1.0`; the installer does not normalize loose versions.

## Dry Run And Roots

`--install-root <PATH>` selects the root used for versioned bundle contents and
the future active pointer. The default plan root is `/opt/m80`, and the active
pointer is planned as `<install-root>/active`.

`--dry-run` renders the validated plan and does not create the install root,
write `/opt`, write `/etc`, write profile files, or switch the active pointer.
For `--release-tag` and `--bootstrap-tag`, dry-run still fetches and verifies
the release asset index so the plan names the concrete selected bundle.

Without `--dry-run`, `--release-tag` and `--bootstrap-tag` fetch the pinned
`m80-release-assets.json` plus its checksum sidecar for the selected tag,
verify the index sha256, then select the Linux x86_64 minimal bundle by
release tag, OS, architecture, and image kind before any bundle download or
extraction. The selected bundle is still staged through the existing
checksum-sidecar downloader; indexed size and digest verification is tracked by
`m80-o3uh9.3.10`. Missing default rows, duplicate defaults, wrong architecture,
wrong image kind, and wrong index asset tags fail with tuple-specific
diagnostics before the install root is touched.

The installer also supports explicit local `file://...` bundle URLs and GitHub
release bundle URLs under
`https://github.com/moradology/m80/releases/download/...`. A successful bundle
install stages and verifies the selected or explicit bundle, copies the
verified layout into `<install-root>/versions/<release_tag>`, writes
install-root-local profile state, and switches `<install-root>/active` last.
Local `http://127.0.0.1`, `http://localhost`, and `http://[::1]` bundle URLs
are accepted only as test-fixture transports for remote staging coverage; they
are not documented as an operator install path.

## Release Identity Checks

`--release-tag` and `--bootstrap-tag` require the running `m80` binary to be a
tagged release build for the same tag. Dev builds fail before any install-root
or active-state write. Tagged binaries whose build tag does not match the
selected source tag also fail before host state changes.

`--bundle-url` is the development and operator override path. A local bundle
URL can be planned and layout-installed from a dev binary. If a GitHub release
bundle URL includes a `/releases/download/<tag>/` segment, a dev binary refuses
it and a tagged release binary must match that tag.

## Tests

Parser and help coverage:

- `parse_install_release_tag_source`
- `parse_install_bundle_url_source_with_root_and_dry_run`
- `parse_install_bootstrap_tag_source`
- `parse_install_missing_source_reaches_command_diagnostic`
- `parse_install_rejects_multiple_sources`
- `help_install`

Integration and behavior-doc coverage:

- `install_dry_run_bundle_url_does_not_touch_install_root`
- `install_json_dry_run_uses_stdout_envelope`
- `install_release_tag_refuses_dev_build_before_install_root_touch`
- `install_missing_source_prints_source_diagnostic`
- `install_non_release_remote_bundle_url_is_rejected_without_touching_install_root`
- `installer_input_contract_doc_names_source_shapes_and_tests`

Module coverage in `cmds/install.rs` pins release-build success, dev-build
refusal, source/binary tag mismatch refusal, tagged release bundle URL refusal
from a dev binary, explicit local bundle dry-run planning from a dev binary,
explicit bundle URL index-bypass behavior, GitHub release-tag extraction from
bundle URLs, bootstrap handoff source selection, and fail-closed index
selection for missing defaults, duplicate defaults, wrong architecture, wrong
image kind, and wrong asset tag entries.
