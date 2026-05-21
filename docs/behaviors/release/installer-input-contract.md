# Installer Input Contract

Behavior bead: `m80-o3uh9.3.1`.

`m80 install` is the release-bundle installer front door. This behavior pins
the user-facing input contract, dry-run plan, and release-bundle layout copy.
The command does not yet perform privileged copies, write `/etc` profile state,
or resolve "latest". Successful bundle installs write install-root-local
profile state and switch the install-root active pointer.

The user-facing trust model for official direct bundle URLs is captured in
[`installer-input.md`](installer-input.md). That document distinguishes the
normal public `install.sh` path from explicit direct bundle URL overrides.

## Source Selection

The installer accepts exactly one source shape:

- `--release-tag <TAG>`: a pinned stable release tag selected by the operator.
  Stable tags are exactly `vMAJOR.MINOR.PATCH`; prerelease suffixes are
  rejected by the normal install path.
- `--bundle-url <URL>`: an explicit release bundle URL, including local
  `file://` bundle fixtures used while developing installer behavior.
- `--bootstrap-tag <TAG>`: a concrete tag handed to `m80 install` by the stable
  bootstrapper after it resolves "latest". This flag is hidden from normal help
  because humans should use `--release-tag` or `--bundle-url`.

The parser rejects more than one source. A missing source reaches the command
validator so the diagnostic names the two human-facing choices without exposing
the hidden bootstrapper handoff. Source tags are exact stable GitHub release
tags such as `v0.1.0`; the installer does not normalize loose versions,
prerelease suffixes, or mutable aliases.

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

The installer also supports explicit local `file://...` bundle URLs and
concrete stable-tag GitHub release bundle URLs under
`https://github.com/moradology/m80/releases/download/<tag>/m80-<target>.tar.gz`.
Mutable `releases/latest/download` artifact URLs, raw branch URLs, foreign
repositories, path-traversal asset paths, non-bundle release assets such as
`install.sh`, and non-HTTPS GitHub release URLs fail before network access or
install-root mutation.

Before an official GitHub release bundle URL can create staging state, the
layout installer resolves the same-tag public material set. It fetches and
checksum-verifies the tag's `m80-release-assets.json`, requires the selected
bundle row URL to match the explicit bundle URL, requires the row's
`attestation_name` to name `m80-release-integrity.attestation.jsonl`, and builds
a plan covering the bundle, bundle checksum sidecar, metadata sidecar,
metadata checksum sidecar, asset index, asset-index checksum, `install.sh`,
`install.sh.sha256`, `m80-bootstrap-selector.tsv`, its checksum sidecar,
`m80-release-build.json`, its checksum sidecar, `m80-release-integrity.json`,
`m80-release-integrity.attestation.jsonl`, `m80-release-attestation.json`, and
public `SHA256SUMS`. The bundle tarball itself is only listed at this stage,
not downloaded. Missing public material, checksum sidecar mismatch, metadata
digest mismatch, or redirect to an unsupported host fails before `<install-root>`
or `.staging` exists. The failure diagnostic names the resolved release tag,
material class, material name, public URL, and expected public digest or
identity without printing credentials or temporary paths.

A successful bundle install stages and verifies the selected or explicit bundle, copies the
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
For release-tag source failures that come from asset-index selection or release
identity, `--json` emits the structured asset-index diagnostic on stderr with
stdout empty. The diagnostic names the requested OS, architecture, image kind,
release tag, m80 version, available alternatives, and repair command when one
is known.

`--bundle-url` is the development and operator override path. A local bundle
URL can be planned and layout-installed from a dev binary. Only concrete
moradology/m80 GitHub release bundle URLs claim an official release tag; local
fixture URLs remain explicit overrides even if their path is release-shaped. If
an official GitHub release bundle URL includes a `/releases/download/<tag>/`
segment, a dev binary refuses it and a tagged release binary must match that
tag.

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
- `install_json_release_tag_refusal_reports_asset_index_fields_on_stderr`
- `install_missing_source_prints_source_diagnostic`
- `install_non_release_remote_bundle_url_is_rejected_without_touching_install_root`
- `install_foreign_github_release_bundle_url_is_rejected_without_touching_install_root`
- `install_latest_artifact_bundle_url_is_rejected_without_touching_install_root`
- `install_prerelease_bundle_tag_is_rejected_without_touching_install_root`
- `install_raw_branch_bundle_url_is_rejected_without_touching_install_root`
- `install_bad_release_asset_name_is_rejected_without_touching_install_root`
- `install_path_traversal_release_asset_url_is_rejected_without_touching_install_root`
- `install_non_https_github_release_url_is_rejected_without_touching_install_root`
- `installer_input_contract_doc_names_source_shapes_and_tests`
- `direct_plan_lists_same_tag_urls_and_expected_identity_before_fetch`
- `direct_plan_rejects_material_name_url_injection`
- `direct_plan_requires_official_attestation_bundle_ref`
- `official_bundle_checksum_redirect_stays_bound_to_same_bundle`
- `official_bundle_redirect_must_stay_on_same_release_asset`
- `official_release_missing_material_fails_before_staging_or_bundle_download`

Module coverage in `cmds/install.rs` pins release-build success, dev-build
refusal, source/binary tag mismatch refusal, tagged release bundle URL refusal
from a dev binary, explicit local bundle dry-run planning from a dev binary,
explicit bundle URL index-bypass behavior, official GitHub release-tag
extraction from bundle URLs, local fixture URLs not claiming release tags,
bootstrap handoff source selection, and fail-closed index selection for missing
defaults, duplicate defaults, wrong architecture, wrong image kind, and wrong
asset tag entries.
