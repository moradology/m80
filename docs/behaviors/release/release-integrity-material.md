# Release Integrity Material

The release integrity material is the typed subject list that signature and
attestation verification binds before an installer extracts a bundle or changes
active install state.

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

Each subject records `name`, `kind`, `sha256`, and `size_bytes`. The subject
set is derived from `m80-release-assets.json`: every row contributes its
bundle, bundle checksum sidecar, metadata sidecar, metadata checksum sidecar,
and any named detached signature. The current single-row default Linux subject
set is:

```text
m80-linux-x86_64.tar.gz
m80-linux-x86_64.tar.gz.sha256
install.sh
install.sh.sha256
m80-linux-x86_64.bundle.json
m80-linux-x86_64.bundle.json.sha256
m80-release-assets.json
m80-release-assets.json.sha256
m80-bootstrap-selector.tsv
m80-bootstrap-selector.tsv.sha256
m80-release-build.json
m80-release-build.json.sha256
SHA256SUMS
```

There are no detached signature files in v1. If a future asset-index row names
`signature_name` or another proof file that the installer consumes, that file
must be added as a subject in the same predicate before signed release
verification accepts the row.

The public `SHA256SUMS` contains every release-integrity subject except
`SHA256SUMS` itself. The GitHub attestation bundle is checked as the proof for
the predicate and is intentionally not a predicate subject.

The v1 asset index still carries both proof-reference fields. Official signed
release rows must set `attestation_name` to
`m80-release-integrity.attestation.jsonl`; `signature_name` must be null unless
a detached signature file is actually published. Signed verification rejects
missing proof-reference fields, empty attestation names, stale attestation
bundle names, absent signature files, stale row release tags or m80 versions,
signature names that collide with built-in subject names, any named signature
file that is not a predicate subject, and any asset-index row missing from the
public checksum manifest.

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

The versioned public `install.sh` is one trusted verifier distribution for its
own release tag. It carries the same repository, signer workflow, issuer,
keyset, and validity-window constants as the policy file, then verifies the
downloaded predicate and attestation bundle before extracting the selected
bundle or handing off to `bin/m80 install`.

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
attestations. The release workflow publishes proof material only for this
matching workflow identity; changing the workflow path is a trust-policy
cutover.

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
`m80-release-integrity.json`. The metadata window follows the m80 trust-policy
acceptance epoch; `gh attestation verify` remains responsible for validating
the short-lived signing certificate, timestamp, and Sigstore roots.

The cryptographic trust check is `gh attestation verify` over
`m80-release-integrity.json`, scoped to `--repo moradology/m80`, the policy
`--signer-workflow`, the policy `--cert-oidc-issuer`, `--source-ref
refs/tags/<release_tag>`, `--source-digest <commit_sha>`, the supplied
attestation bundle, and `--deny-self-hosted-runners`. The normalized metadata
is policy input and audit material; it is not treated as a signature substitute.

Before an official signed install or human verification command reads release
proof material, it preflights the selected attestation verifier. The v1
preflight runs `gh --version` and `gh attestation verify --help`, then requires
support for `--repo`, `--bundle`, `--signer-workflow`,
`--cert-oidc-issuer`, `--source-ref`, `--source-digest`,
`--deny-self-hosted-runners`, and `--format`. Missing or too-old verifier
support fails before network fetch, extraction, `install.sh` execution, sudo, or
active-state writes. The failure names the selected tool, observed version/help
output when available, why attestation verification is required, and the Linux
remediation: Install or upgrade GitHub CLI with attestation support.

## Verification Contract

[`release-integrity-contract.json`](release-integrity-contract.json) is the
checked shared contract fixture for the default Linux tuple. It names the
required release material files, role classes, predicate subject kinds,
`SHA256SUMS` membership, and attestation signer/issuer/keyset constants that
both the human/public installer verifier and the Rust direct-URL verifier must honor.
Changing a required material role or subject kind must update that fixture and
the paired verifier tests in the same diff; otherwise one verifier lane can
silently become weaker than the other.

`scripts/verify-release-bundle.py --verify-integrity` is the one-command human
verifier for a downloaded public release dist directory. It first verifies the
tarball contract plus adjacent public checksum sidecars, then invokes
`scripts/verify-release-integrity.py` as the shared trust-anchor loader for
human verification and installer verification. The integrity verifier checks
the predicate, trust policy, and attestation metadata against the same release
dist directory. It fails closed when:

- `schema_version` is unsupported;
- `mechanism` is not `github-artifact-attestation`;
- `repository` is not `moradology/m80`;
- `release_tag`, `commit_sha`, `target`, `rust_toolchain`, or
  `m80_package_version` do not match the expected release inputs;
- `m80-release-build.json` does not match the signed release tag, source
  commit, Rust toolchain, target, package version, bundle metadata hash, or
  current `Cargo.lock` digest;
- the build manifest omits target triples, records neither apt package
  versions nor a container digest, or records a container digest that is not
  `sha256:<64 lowercase hex>`;
- trust policy, attestation bundle, or attestation metadata is missing;
- trust policy or attestation metadata uses an unsupported schema;
- trust policy or attestation metadata uses a different repository, mechanism,
  release tag, predicate hash, signer, issuer, or keyset;
- trust policy is not yet active or has expired;
- attestation certificate/key material is not yet active or has expired;
- rotation mode is not `hard-fail-expired`;
- the selected attestation verifier is missing or does not support the required
  `gh attestation verify` flags;
- `gh attestation verify` cannot cryptographically verify the predicate for
  the pinned repo, signer, tag ref, commit SHA, and bundle;
- the subject set is missing, duplicated, or has unknown fields;
- a subject omits its digest or size;
- a subject file is missing;
- a subject digest or size does not match current bytes;
- `bundle_metadata_sha256` does not match the metadata sidecar;
- bundle metadata or the asset index names a different release tag;
- an installer-consumed asset-index row omits proof-reference fields, leaves
  `attestation_name` empty, points at stale or absent proof material, names a
  stale m80 version, or names a signature file omitted from the predicate;
- the bootstrap selector names a different release tag or drifts from the
  asset index tuple map.

The signed-material bootstrap selector negative matrix covers unsupported
selector schema, missing tuple rows, duplicate tuple, extra tuple, stale
release tag, stale bundle digest, stale size, row-shape mismatch, and
non-shell-safe selector token characters. Each case rewrites the selector
sidecar and then regenerates `m80-release-integrity.json`, so failures must
come from semantic selector verification rather than a stale subject digest.

Installer and bootstrapper verification must run this contract before
extracting a bundle, running `install.sh`, or writing active install state. A
checksum-only verifier may run earlier, but it is not a replacement for this
predicate once signed/attested material is required.
The versioned `install.sh` embedded verifier also validates
`m80-release-build.json` semantics before bundle extraction, including release
tag, source commit, Rust toolchain, target, package version, target triples,
builder material, and bundle metadata hash.

The lower-level `m80 install --bundle-url
https://github.com/moradology/m80/releases/download/<tag>/<bundle>.tar.gz`
path is not a weaker direct-artifact trust mode. For concrete official
`moradology/m80` bundle URLs, the CLI verifies the same-tag asset index row,
bundle checksum sidecar, bundle bytes, bundle metadata sidecar, `install.sh`
and sidecar, bootstrap selector and sidecar, build manifest and sidecar, public
`SHA256SUMS`, release-integrity predicate subjects, and normalized attestation
metadata before creating the install-root staging directory or listing the
tarball. It also runs `gh attestation verify` over
`m80-release-integrity.json` using the downloaded
`m80-release-integrity.attestation.jsonl` bundle, scoped to the same
repository, signer workflow, issuer, tag ref, commit digest, self-hosted-runner
denial, and JSON output policy as the public installer. The attestation bundle
is never accepted because it exists beside the predicate; it must verify the
predicate subject name and digest before the selected bundle bytes are
downloaded.

Redirect validation is an identity check plus the digest/proof checks above,
not a GitHub CDN host allowlist. Every official release material fetch starts
from the expected `moradology/m80` repository, concrete release tag, and
expected asset name. If curl reports a final GitHub release-asset URL, that
final URL must carry the same repository, tag, and asset name. Opaque GitHub
asset CDN URLs are accepted only as final redirects for that exact requested
release asset, and the downloaded bytes remain untrusted until the material's
checksum sidecar, public `SHA256SUMS` row, predicate subject, or asset-index
identity check succeeds. Foreign repositories, wrong tags, wrong asset names,
host lookalikes, and digest-matching wrong-role release paths fail closed with
the material role, requested URL, final URL, expected asset name, and rejected
identity field in the diagnostic.

Any digest, subject, asset-index, attestation-metadata,
cryptographic attestation, or missing `install.sh` row disagreement fails while
the verified bundle still lives only in a temporary release-material directory.
Those CLI failures name the failed `material_class`, print a safe
`retry_command=m80 install --bundle-url '<url>' --install-root '<path>'`, and
leave the install root byte-for-byte unchanged: no staging cleanup, no profile
write, no version publication, no active pointer mutation, and no tar listing
or extraction before the release-material contract is complete.
Local `file://` and local test fixtures remain explicit operator overrides and
do not claim official release trust.

`scripts/verify-install-handoff.py` is the narrower pre-root gate for
automation that already has a trusted m80 checkout. It downloads no assets by
itself; given local `install.sh`, `install.sh.sha256`,
`m80-release-integrity.json`, `m80-release-integrity.attestation.jsonl`, and
`m80-release-attestation.json`, it verifies the installer checksum and the
signed predicate subject before printing the local `sudo sh <tmp>/install.sh`
handoff command. The full installer still verifies the complete bundle and
asset-index contract before extraction or active-state writes.

## Human Command

A human can verify downloaded release material without installing it:

This command requires the release dist to include the attestation bundle and
normalized attestation metadata. If those files are absent, the release is not
signed for installer purposes and verification must fail closed. The trust
policy comes from a trusted m80 checkout or installed verifier distribution,
not from the downloaded release dist being verified.

```sh
M80_RELEASE_COMMIT="$(git rev-list -n 1 "$M80_RELEASE_TAG")"

python3 scripts/verify-release-bundle.py \
  /tmp/m80-release-dist/m80-linux-x86_64.tar.gz \
  --release-tag "$M80_RELEASE_TAG" \
  --verify-integrity \
  --commit-sha "$M80_RELEASE_COMMIT" \
  --trust-policy docs/behaviors/release/m80-release-trust-policy.json \
  --attestation-bundle /tmp/m80-release-dist/m80-release-integrity.attestation.jsonl \
  --attestation-metadata /tmp/m80-release-dist/m80-release-attestation.json \
  --verification-time "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
  --rust-toolchain 1.82
```

The command is read-only and does not require root.

`--verify-integrity` implies public sidecar verification. It checks the bundle
tar internals, bundle checksum, installer checksum, metadata sidecar, asset
index, bootstrap selector, build manifest, public `SHA256SUMS`, signed
predicate, attestation metadata, cryptographic attestation bundle, and tag
identity before printing success.

If this command fails before reading release files with an attestation-verifier
error, upgrade GitHub CLI to a build that includes `gh attestation verify`.
Linux package instructions are at <https://cli.github.com/packages>.

## Coverage

`scripts/test-release-bundle.py` covers:

- `test_release_integrity_material_verifier_accepts_valid_fixture`;
- `test_human_release_dist_verifier_accepts_clean_public_dist`;
- `test_human_release_dist_verifier_rejects_tampered_tarball`;
- `test_human_release_dist_verifier_rejects_tampered_install_sh`;
- `test_human_release_dist_verifier_rejects_wrong_tag`;
- `test_human_release_dist_verifier_rejects_missing_attestation`;
- `test_human_release_dist_verifier_rejects_missing_sidecar`;
- `test_release_integrity_material_rejects_missing_asset_index_attestation_ref`;
- `test_release_integrity_material_rejects_empty_asset_index_attestation_ref`;
- `test_release_integrity_material_rejects_stale_asset_index_attestation_ref`;
- `test_release_integrity_material_rejects_attestation_bundle_path_name_mismatch`;
- `test_release_integrity_material_rejects_stale_asset_index_release_tag`;
- `test_release_integrity_material_rejects_stale_asset_index_m80_version`;
- `test_release_integrity_material_rejects_named_signature_without_subject`;
- `test_release_integrity_material_rejects_absent_asset_index_signature_ref`;
- `test_release_integrity_material_rejects_signature_name_colliding_with_subject`;
- `test_release_integrity_material_accepts_named_signature_subject`;
- `test_release_workflow_publishes_and_verifies_proof_assets`;
- `test_release_attestation_metadata_writer_accepts_verified_bundle`;
- `test_release_integrity_material_preflights_missing_verifier_before_material_read`;
- `test_release_attestation_metadata_writer_preflights_missing_verifier_before_material_read`;
- `test_release_integrity_material_rejects_too_old_attestation_verifier`;
- `test_release_integrity_material_rejects_attestation_verifier_missing_required_flag`;
- `test_rendered_install_script_selects_verified_selector_before_bundle`;
- `test_rendered_install_script_rejects_unsigned_dev_fixture_before_bundle`;
- `test_rendered_install_script_rejects_tampered_install_before_bundle_extract`;
- `test_rendered_install_script_rejects_wrong_integrity_tag_before_bundle_extract`;
- `test_rendered_install_script_rejects_build_manifest_commit_mismatch_before_bundle`;
- `test_rendered_install_script_rejects_build_manifest_metadata_hash_mismatch_before_bundle`;
- `test_rendered_install_script_rejects_build_manifest_target_triples_mismatch_before_bundle`;
- `test_rendered_install_script_rejects_build_manifest_rust_toolchain_mismatch_before_bundle`;
- `test_rendered_install_script_rejects_build_manifest_target_mismatch_before_bundle`;
- `test_rendered_install_script_rejects_build_manifest_package_version_mismatch_before_bundle`;
- `test_rendered_install_script_rejects_build_manifest_without_builder_material_before_bundle`;
- `test_rendered_install_script_rejects_build_manifest_malformed_container_digest_before_bundle`;
- `test_rendered_install_script_rejects_failed_attestation_before_bundle_extract`;
- `direct_plan_lists_same_tag_urls_and_expected_identity_before_fetch`;
- `direct_plan_rejects_material_name_url_injection`;
- `direct_plan_requires_official_attestation_bundle_ref`;
- `official_release_missing_attestation_verifier_fails_before_staging`;
- `official_release_missing_material_fails_before_staging_or_bundle_download`;
- `official_release_too_old_attestation_verifier_fails_before_staging`;
- `missing_integrity_predicate_aborts_before_install_root_mutation`;
- `missing_attestation_bundle_aborts_before_install_root_mutation`;
- `missing_asset_index_aborts_before_install_root_mutation`;
- `missing_public_sha256s_aborts_before_install_root_mutation`;
- `missing_checksum_sidecar_aborts_before_install_root_mutation`;
- `trust_policy_signer_mismatch_aborts_before_install_root_mutation`;
- `public_sha256s_digest_mismatch_aborts_before_install_root_mutation`;
- `integrity_predicate_digest_mismatch_aborts_before_install_root_mutation`;
- `asset_index_digest_mismatch_aborts_before_install_root_mutation`;
- `cryptographic_attestation_failure_aborts_before_install_root_mutation`;
- `test_package_assembles_multi_tuple_release_from_tuple_manifest`;
- `test_package_rejects_extra_tuple_name_collision_before_copy`;
- `test_package_rejects_extra_tuple_metadata_sidecar_mismatch`;
- `test_release_integrity_material_accepts_complete_public_subject_set`;
- `test_release_integrity_material_rejects_wrong_tag`;
- `test_release_integrity_material_rejects_missing_asset_hash`;
- `test_release_integrity_material_rejects_missing_install_subject`;
- `test_release_integrity_material_rejects_missing_asset_index_subject`;
- `test_release_integrity_material_rejects_missing_bootstrap_selector_subject`;
- `test_release_integrity_material_rejects_missing_build_manifest_subject`;
- `test_release_integrity_material_rejects_build_manifest_commit_mismatch`;
- `test_release_integrity_material_rejects_build_manifest_malformed_container_digest`;
- `test_release_integrity_material_rejects_unexpected_extra_subject`;
- `test_release_integrity_material_rejects_subject_digest_mismatch`;
- `test_release_integrity_material_rejects_tampered_bundle_hash`;
- `test_release_integrity_material_rejects_tampered_install_hash`;
- `test_release_integrity_material_rejects_signed_bootstrap_selector_unsupported_schema`;
- `test_release_integrity_material_rejects_signed_bootstrap_selector_missing_tuple_rows`;
- `test_release_integrity_material_rejects_signed_bootstrap_selector_duplicate_tuple`;
- `test_release_integrity_material_rejects_signed_bootstrap_selector_extra_tuple`;
- `test_release_integrity_material_rejects_signed_bootstrap_selector_stale_tag`;
- `test_release_integrity_material_rejects_signed_bootstrap_selector_stale_digest`;
- `test_release_integrity_material_rejects_signed_bootstrap_selector_drift`;
- `test_release_integrity_material_rejects_signed_bootstrap_selector_row_shape_mismatch`;
- `test_release_integrity_material_rejects_signed_bootstrap_selector_shell_metacharacters`;
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
- `test_release_integrity_material_rejects_replayed_repo_attestation`;
- `test_release_integrity_contract_fixture_matches_python_verifier`;
- `direct_release_material_plan_matches_shared_integrity_contract`.
