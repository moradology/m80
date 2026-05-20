# Release Integrity Material

The release integrity material is the typed subject list that future signature
and attestation verification must bind before an installer extracts a bundle or
changes active install state.

The v1 release integrity schema uses GitHub Artifact Attestations as the public
provenance mechanism. The attested predicate is a JSON document named
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

## Verification Contract

`scripts/verify-release-integrity.py` verifies the predicate against a release
dist directory. It fails closed when:

- `schema_version` is unsupported;
- `mechanism` is not `github-artifact-attestation`;
- `repository` is not `moradology/m80`;
- `release_tag`, `commit_sha`, `target`, `rust_toolchain`, or
  `m80_package_version` do not match the expected release inputs;
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

```sh
python3 scripts/verify-release-integrity.py \
  /tmp/m80-release-dist/m80-release-integrity.json \
  --dist-dir /tmp/m80-release-dist \
  --release-tag "$M80_RELEASE_TAG" \
  --commit-sha "$GITHUB_SHA" \
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
- `test_release_integrity_material_rejects_unsupported_verifier_version`.
