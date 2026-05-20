# Installer Input Contract

Behavior bead: `m80-o3uh9.3.1`.

`m80 install` is the release-bundle installer front door. This behavior pins
the user-facing input contract, dry-run plan, and the currently supported
explicit bundle layout copy. The command does not yet perform privileged
copies, write `/etc` profile state, resolve "latest", or switch the active
install pointer.

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

Without `--dry-run`, the active installer supports explicit local `file://...`
bundle URLs and GitHub release bundle URLs under
`https://github.com/moradology/m80/releases/download/...`. A successful bundle
install stages and verifies the bundle, copies the verified layout into
`<install-root>/versions/<release_tag>`, and still does not switch
`<install-root>/active` or write profile state. Release-tag/bootstrap
resolution exits with the unsupported-operation code before creating the
install root until the asset-index/bootstrapper leaves wire source selection.
Local `http://127.0.0.1`, `http://localhost`, and `http://[::1]` bundle URLs are
accepted only as test-fixture transports for remote staging coverage; they are
not documented as an operator install path.

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
and GitHub release-tag extraction from bundle URLs.
