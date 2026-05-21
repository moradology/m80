# Install State

Behavior beads: `m80-o3uh9.16.8.1`, `m80-o3uh9.16.8.2`,
`m80-o3uh9.16.8.3`.

Installed state is rooted under one versioned directory,
`<install-root>/versions/<release_tag>`:

```text
<install-root>/versions/<release_tag>/
```

The active install is selected by `<install-root>/active`, an absolute symlink
that points at one versioned directory. Status readers treat the active pointer
as the local install selector; they do not infer trust from transient download
directories, workflow logs, or current GitHub release pages.

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
