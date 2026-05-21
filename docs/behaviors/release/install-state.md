# Install State

Behavior beads: `m80-o3uh9.16.8.1`, `m80-o3uh9.16.8.2`,
`m80-o3uh9.16.8.3`, `m80-o3uh9.16.7.1`,
`m80-o3uh9.16.7.2`, `m80-o3uh9.16.7.3`,
`m80-o3uh9.16.7.4`, `m80-o3uh9.16.7.5`.

Installed state is rooted under one versioned directory,
`<install-root>/versions/<release_tag>`:

```text
<install-root>/versions/<release_tag>/
```

The active install is selected by `<install-root>/active`, an absolute symlink
that points at one versioned directory. Status readers treat the active pointer
as the local install selector; they do not infer trust from transient download
directories, workflow logs, or current GitHub release pages.

## Active Install Resolver

The resolver for `m80-o3uh9.16.7.2` is a read-only local status primitive. It
reads `<install-root>/active`, the selected version directory, effective
`default_profile` config, and the selected runtime profile. It does not execute
installed binaries, does not run preflight, does not fetch release metadata, and
does not download bundles.

The resolver returns one typed state:

- `healthy_active_release`: config selects the installed profile and
  `<install-root>/active` points at the same version directory.
- `missing_active_pointer`: the selected profile is install-shaped but
  `<install-root>/active` is absent.
- `dangling_active_pointer`: `<install-root>/active` points at a missing
  version directory.
- `local_dev_tree`: the selected profile is the built-in `env` profile, so
  artifact paths come from environment/default discovery instead of an installed
  release tree.
- `stale_profile_target`: the selected profile references one version
  directory while `<install-root>/active` points at another.
- `explicit_override`: an operator override such as `M80_DEFAULT_PROFILE`, a
  user config layer, a config drop-in, or a future `--profile` caller override
  selects a profile instead of trusting the installed system config alone.
- `missing_install_metadata`: the active/profile state is coherent, but a
  required installed metadata file is absent.
- `stale_install_metadata`: installed metadata parses, but one of its recorded
  paths, sizes, release tags, or digests no longer matches the active version
  directory.
- `tampered_proof_cache`: the saved public proof-cache manifest or one of its
  referenced proof files no longer matches the recorded digest.
- `invalid_install_metadata`: config/profile parsing failed, the active pointer
  is malformed, or persisted installed-profile paths are not valid install-root
  paths.

Diagnostics are structured by code, field, path, and message. The resolver
rejects active-pointer traversal (`..`), active targets outside
`<install-root>/versions/`, active targets that are not exactly one version
directory, install-owned profile path traversal, and install-owned profile
paths outside the install root. Host prerequisite paths such as Firecracker and
jailer binaries may still point at their documented system locations. Explicit
operator profile overrides are reported as overrides rather than rejected as
broken installed metadata.

## Installed Status Command

`m80 install-status` is the smallest operator-facing wrapper around the
resolver. It is read-only: it does not execute installed binaries, run
preflight, fetch release metadata, repair profiles, or download bundles.

Human output is line-oriented and includes:

- `status`: one resolver state from the finite list above.
- `active_release_tag`, `active_install_dir`, `active_pointer_path`, and
  `active_pointer_target`.
- `selected_config_default_profile` and
  `selected_config_default_profile_source`.
- `selected_profile`, `selected_profile_source`,
  `selected_profile_body_source`, `selected_profile_path`,
  `selected_profile_artifact_dir`, `selected_profile_install_dir`, and
  `selected_profile_release_tag`.
- `bundle_metadata_path`, `host_binaries_manifest_path`,
  `install_provenance_path`, and `proof_cache_manifest_path`, each paired with
  a metadata status. When the active/profile state is too broken to read
  metadata, these fields render as `unavailable`.
- `next_action`, plus `next_action_command` when the state has an executable
  repair or smoke command.

`m80 --json install-status` wraps the same contract in the CLI JSON envelope
with `schema_version: 1`. The stable top-level payload fields are `status`,
`install_root`, `active`, `selected_config`, `selected_profile`, `metadata`,
`diagnostics`, and `next_action`. `status` uses the resolver state enum;
`active.status` uses `live`, `missing`, `dangling`, or `invalid`; metadata file
statuses use `present`, `missing`, `invalid`, or `stale` when metadata is
available; and
`next_action.kind` uses `ready`, `install_release`, `reinstall_release`, or
`remove_override`.

Status output also includes `mismatches`, a structured list for cases where the
selected local state is intentionally or accidentally not the installed default.
Each mismatch has a finite `code`, a message, and any applicable
`expected_path`, `observed_path`, `expected_tag`, `observed_tag`,
`expected_source`, `observed_source`, `expected_value`, and `observed_value`.
The status command uses this list to make these cases distinct:

- `stale_profile_target`: the default installed profile points at one release
  directory while `<install-root>/active` points at another. The expected fields
  name the active pointer release tag and directory; the observed fields name
  the selected profile release tag and directory.
- `explicit_profile_override`: an environment variable, flag, user config, or
  drop-in selected a profile instead of the installed system config. This is
  intentional override state, not a stale default profile.
- `selected_profile_unavailable`: effective config names a default profile, but
  that profile cannot be resolved.
- `install_root_override`: status is inspecting a non-default install root. By
  default, with no override, `m80 install-status` inspects `/opt/m80`, reads the
  installed system config, and expects that config's default profile to point at
  the same version directory as `/opt/m80/active`.

Quickstart troubleshooting starts with `m80 install-status` to decide whether
the local release bundle/profile pair is coherent. Host substrate problems stay
in `m80 preflight`; update freshness stays in the release freshness monitor.

## Installed Metadata Reader

The metadata reader for `m80-o3uh9.16.7.3` is the status-facing view of the
active version directory. It parses these files without running binaries,
preflight, network fetches, or installer repair:

- `<version-dir>/bundle.json`
- `<version-dir>/artifacts/install-provenance.json`
- `<version-dir>/artifacts/host-binaries.manifest.json`
- `<version-dir>/artifacts/release-proof-cache/manifest.json`

The reader treats unknown JSON fields, malformed digests, absolute or escaping
bundle file paths, malformed proof-cache file names, symlinked installed references,
and missing referenced files as unhealthy installed state. Metadata
files are opened with `O_NOFOLLOW`, referenced bundle/proof/provenance files
are hashed from an opened file descriptor instead of read fully into memory, and
relative installed references must not traverse symlink components inside the
active version directory. Bundle metadata and proof-cache DTOs use
`deny_unknown_fields`; host-binaries and install-provenance parsing reuses the
shared manifest readers from `m80-image-manifest`. A missing metadata file
reports `missing_install_metadata`; a stale bundle/provenance reference reports
`stale_install_metadata`; a stale proof-cache reference reports
`tampered_proof_cache`.

## Proof Cache Contract

Official release installs preserve the public proof material under
`<install-root>/versions/<release_tag>/artifacts/release-proof-cache/`:

```text
<install-root>/versions/<release_tag>/artifacts/release-proof-cache/
```

The cache manifest is
`<install-root>/versions/<release_tag>/artifacts/release-proof-cache/manifest.json`:

```text
<install-root>/versions/<release_tag>/artifacts/release-proof-cache/manifest.json
```

`manifest.json` has `schema_version: 1`, a top-level `manifest_digest`, and a
`payload` object. `manifest_digest` is the sha256 of the canonical JSON payload,
not a digest of the enclosing manifest object. The payload records what the
installer verified; it is evidence for offline status and repair diagnostics,
not a second trust root.

The payload fields are:

- `release_tag`, `repository`, and `target`;
- `integrity_predicate`: path, sha256, and size for
  `m80-release-integrity.json`;
- `attestation_bundle`: path, sha256, and size for
  `m80-release-integrity.attestation.jsonl`;
- `attestation_metadata`: path, sha256, size, signer identity, issuer, keyset,
  and predicate sha256 for `m80-release-attestation.json`;
- `asset_index`: path, sha256, and size for `m80-release-assets.json`;
- `public_sha256s`: path, sha256, and size for public `SHA256SUMS`;
- `checksum_sidecars`: every public checksum sidecar path, sha256, and subject;
- `trust_policy`: policy path, trust identity, and policy sha256;
- `verifier_versions`: m80 version, GitHub CLI version, release-integrity schema
  version, and asset-index schema version.

The parser is typed and uses `deny_unknown_fields`. Missing required fields,
unknown fields, empty path/identity strings, zero sizes, unsupported schema
versions, malformed sha256 fields, and mismatched `manifest_digest` all fail
closed before any status surface trusts the cached proof.

## Transaction Ordering

For official release installs, the installer writes the proof cache inside the
staged version directory after extraction and installed metadata rewriting, but
before the staged tree is renamed to `<install-root>/versions/<release_tag>`,
before default profile/config writes, and before `<install-root>/active` is
renamed. The writer copies the verified public material, writes the trust
policy used by the verifier, computes the manifest digest, reads the manifest
back through the typed parser, and checks the cache directory/file modes before
the install can proceed to version-directory publication, host-binaries manifest
generation, and profile publishing.

If proof-cache file creation, manifest rehashing, or mode checking fails, the
staged tree is not renamed into the version directory and is not active. Any
previous active pointer, default profile, and config file remain selected. A
pre-existing non-directory proof-cache target is rejected without rewriting the
colliding path.

## Tests

- `complete_manifest_parses_and_validates_digest`
- `missing_required_field_fails_closed`
- `unknown_field_fails_closed`
- `malformed_material_digest_fails_closed`
- `malformed_manifest_digest_fails_closed`
- `write_verified_release_proof_cache_copies_manifest_and_mode_checks_material`
- `write_verified_release_proof_cache_rejects_existing_cache_target_file`
- `proof_cache_write_failure_leaves_previous_active_profile_and_config_selected`
- `proof_cache_manifest_digest_failure_leaves_previous_active_profile_and_config_selected`
- `proof_cache_mode_failure_leaves_previous_active_profile_and_config_selected`
- `install_state_doc_names_proof_cache_manifest_contract`
- `status_matrix_healthy_active_release`
- `status_matrix_missing_active_pointer`
- `status_matrix_dangling_active_pointer`
- `status_matrix_stale_profile_target`
- `status_matrix_explicit_override`
- `status_matrix_explicit_override_flag_source`
- `status_matrix_local_dev_tree`
- `status_matrix_tampered_proof_cache`
