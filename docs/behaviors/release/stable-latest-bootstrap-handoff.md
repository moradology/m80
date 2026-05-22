# Stable Latest Bootstrap Handoff

Bead: `m80-o3uh9.11.3`

The stable latest bootstrapper is only a resolver and handoff producer. It
loads GitHub latest release metadata, validates that the release is stable and
complete, checks the latest metadata again as a tag-switch guard, and then
emits pinned inputs for the versioned install path.

The handoff JSON has three operator-facing contract fields:

- `versioned_install_args`: arguments for downstream versioned install logic.
  These args contain `--bootstrap-tag <resolved-tag>` and optional
  `--install-root <path>`. They must not contain `latest` or a mutable
  `/releases/latest/` URL.
- `versioned_install_inputs`: the full pinned handoff material: resolved
  release tag, optional install root, asset-index URL and optional local
  validation path, all public asset URLs, checksum URLs, and proof URLs. URLs
  must be pinned to `releases/download/<resolved-tag>/...`.
- `bootstrap_proof`: the resolved tag and every pinned public asset URL used
  for the install. Each URL uses `releases/download/<resolved-tag>/...`.

The bootstrapper fails before emitting handoff JSON when the latest metadata
changes between the initial read and guard read, or when a required pinned
asset such as `install.sh` is missing from the release metadata. Downstream
install logic never receives mutable latest inputs from this bootstrap step.

Regression coverage lives in `scripts/test-stable-latest-bootstrap.py`:

- `test_successful_handoff_args_are_pinned_and_latest_free`
- `test_rejects_latest_tag_switch_before_emitting_handoff_json`
- `test_rejects_missing_pinned_install_sh_before_handoff_json`
- `test_url_mode_fetches_latest_twice_and_emits_no_mutable_latest_url`
