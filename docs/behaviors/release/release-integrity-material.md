# Release Integrity Material

The release integrity material is the typed subject list that future signature
and attestation verification must bind before an installer extracts a bundle or
changes active install state.

The v1 release integrity schema uses GitHub Artifact Attestations as the sole
public provenance mechanism. The attested predicate is a JSON document named
`m80-release-integrity.json` with `schema_version: 1`. Detached signatures are
not part of this schema. If a future release adds a detached signature lane, it
must sign the same predicate rather than defining a second subject list.

The predicate records:

- `mechanism: "github-artifact-attestation"`;
- `repository: "moradology/m80"`;
- `release_tag`;
- `commit_sha`;
- `target`, currently `linux-x86_64`;
- `rust_toolchain`;
- `m80_package_version`;
- `bundle_metadata_name`;
- `bundle_metadata_sha256`;
- `subjects`.

Each subject records `name`, `kind`, `sha256`, and `size_bytes`. The current
public subject set is:

```text
m80-linux-x86_64.tar.gz
m80-linux-x86_64.tar.gz.sha256
install.sh
install.sh.sha256
m80-linux-x86_64.bundle.json
m80-linux-x86_64.bundle.json.sha256
m80-release-assets.json
m80-release-assets.json.sha256
SHA256SUMS
```

The bundle metadata hash is duplicated as `bundle_metadata_sha256` because
installers and human verifiers need to bind the tarball metadata before
extraction.

## Trust Root

The release trust root is explicit verifier metadata, not a file trusted merely
because it arrived beside release assets. Human verification and installer
verification both load the same anchored policy from the trusted verifier
distribution:

- `docs/behaviors/release/m80-release-trust-policy.json`, the policy/keyset
  anchor;
- `m80-release-integrity.attestation.jsonl`, the GitHub Artifact Attestation
  bundle for `m80-release-integrity.json`;
- `m80-release-attestation.json`, the normalized GitHub Artifact Attestation
  envelope for `m80-release-integrity.json`.

The v1 trust policy records:

- `schema_version: 1`;
- `mechanism: "github-artifact-attestation"`;
- `repository: "moradology/m80"`;
- `keyset_id`, currently `github-actions-oidc:m80-release-v1`;
- `valid_from` and `valid_until`;
- `allowed_signers`;
- `rotation`.

The current allowed signer is:

```text
identity: moradology/m80/.github/workflows/release-artifacts.yml
issuer: https://token.actions.githubusercontent.com
```

The signer identity is the GitHub Actions workflow allowed to create release
attestations. A future release workflow must publish the matching workflow
identity in the trust policy before that release is verified.

`keyset_id` names the policy epoch for the GitHub/Sigstore trust material used
by `gh attestation verify`. The trust-policy file comes from the trusted
verifier distribution, while `gh` loads and verifies the cryptographic root.
Verifiers compare the normalized attestation `keyset_id` to the policy
`keyset_id` exactly so rotation remains explicit in release material. Rotation
is a hard cutover:

- `rotation.mode` must be `hard-fail-expired`;
- `rotation.overlap_days` names the planned acceptance overlap;
- `rotation.next_keyset_id` is either null or the exact next keyset;
- verification fails once `valid_until` has passed, even if all asset hashes
  still match.

`m80-release-attestation.json` records the attestation facts that the verifier
must bind before trusting the predicate:

- `schema_version: 1`;
- `mechanism: "github-artifact-attestation"`;
- `repository`;
- `release_tag`;
- `predicate_sha256`;
- `signer_identity`;
- `issuer`;
- `keyset_id`;
- `certificate_not_before`;
- `certificate_not_after`.

The attestation repository and tag must match the predicate and the requested
release tag. `predicate_sha256` must be the current sha256 of
`m80-release-integrity.json`. The certificate window and trust-policy window
must both include the verifier's `--verification-time`.

The cryptographic trust check is `gh attestation verify` over
`m80-release-integrity.json`, scoped to `--repo moradology/m80`, the policy
`--signer-workflow`, the policy `--cert-oidc-issuer`, `--source-ref
refs/tags/<release_tag>`, `--source-digest <commit_sha>`, the supplied
attestation bundle, and `--deny-self-hosted-runners`. The normalized metadata
is policy input and audit material; it is not treated as a signature substitute.

## Verification Contract

`scripts/verify-release-integrity.py` is the trust-anchor loader for both human
verification and installer verification. It verifies the predicate, trust
policy, and attestation metadata against a release dist directory. It fails
closed when:

- `schema_version` is unsupported;
- `mechanism` is not `github-artifact-attestation`;
- `repository` is not `moradology/m80`;
- `release_tag`, `commit_sha`, `target`, `rust_toolchain`, or
  `m80_package_version` do not match the expected release inputs;
- trust policy, attestation bundle, or attestation metadata is missing;
- trust policy or attestation metadata uses an unsupported schema;
- trust policy or attestation metadata uses a different repository, mechanism,
  release tag, predicate hash, signer, issuer, or keyset;
- trust policy is not yet active or has expired;
- attestation certificate/key material is not yet active or has expired;
- rotation mode is not `hard-fail-expired`;
- `gh attestation verify` cannot cryptographically verify the predicate for
  the pinned repo, signer, tag ref, commit SHA, and bundle;
- the subject set is missing, duplicated, or has unknown fields;
- a subject omits its digest or size;
- a subject file is missing;
- a subject digest or size does not match current bytes;
- `bundle_metadata_sha256` does not match the metadata sidecar;
- bundle metadata or the asset index names a different release tag.

Installer and bootstrapper verification must run this contract before
extracting a bundle, running `install.sh`, or writing active install state. A
checksum-only verifier may run earlier, but it is not a replacement for this
predicate once signed/attested material is required.

## Human Command

A human can verify downloaded release material without installing it:

This command requires the release dist to include the attestation bundle and
normalized attestation metadata. If those files are absent, the release is not
signed for installer purposes and verification must fail closed. The trust
policy comes from a trusted m80 checkout or installed verifier distribution,
not from the downloaded release dist being verified.

```sh
python3 scripts/verify-release-integrity.py \
  /tmp/m80-release-dist/m80-release-integrity.json \
  --dist-dir /tmp/m80-release-dist \
  --release-tag "$M80_RELEASE_TAG" \
  --commit-sha "$GITHUB_SHA" \
  --trust-policy docs/behaviors/release/m80-release-trust-policy.json \
  --attestation-bundle /tmp/m80-release-dist/m80-release-integrity.attestation.jsonl \
  --attestation-metadata /tmp/m80-release-dist/m80-release-attestation.json \
  --verification-time "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
  --rust-toolchain 1.82
```

The command is read-only and does not require root.

## Coverage

`scripts/test-release-bundle.py` covers:

- `test_release_integrity_material_verifier_accepts_valid_fixture`;
- `test_release_integrity_material_rejects_wrong_tag`;
- `test_release_integrity_material_rejects_missing_asset_hash`;
- `test_release_integrity_material_rejects_tampered_bundle_hash`;
- `test_release_integrity_material_rejects_tampered_install_hash`;
- `test_release_integrity_material_rejects_unsupported_verifier_version`;
- `test_release_integrity_material_rejects_missing_attestation_metadata`;
- `test_release_integrity_material_rejects_missing_trust_policy`;
- `test_release_integrity_material_rejects_missing_attestation_bundle`;
- `test_release_integrity_material_rejects_unsupported_trust_policy_schema`;
- `test_release_integrity_material_rejects_unsupported_attestation_metadata_schema`;
- `test_release_integrity_material_rejects_trust_policy_mechanism_mismatch`;
- `test_release_integrity_material_rejects_failed_cryptographic_attestation`;
- `test_release_integrity_material_rejects_attestation_without_material_subject`;
- `test_release_integrity_material_rejects_attestation_subject_digest_mismatch`;
- `test_release_integrity_material_rejects_attestation_subject_name_mismatch`;
- `test_release_integrity_material_rejects_unknown_signer`;
- `test_release_integrity_material_rejects_stale_keyset`;
- `test_release_integrity_material_rejects_expired_certificate_window`;
- `test_release_integrity_material_rejects_expired_trust_policy`;
- `test_release_integrity_material_rejects_boolean_rotation_overlap`;
- `test_release_integrity_material_rejects_replayed_tag_attestation`;
- `test_release_integrity_material_rejects_replayed_repo_attestation`.
