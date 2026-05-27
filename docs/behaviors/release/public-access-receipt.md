# Public Access Release Receipt

`release-readiness-public-access.json` is the no-auth lane proof for the
release readiness gate. It is built from the public GitHub API and public
download URLs with `GH_TOKEN` and `GITHUB_TOKEN` unset and an empty
`GH_CONFIG_DIR`, so GitHub CLI credentials cannot make a private or stale
release look healthy.

The receipt is valid only when:

- `lane_id` is `public-access-latest`, `proof_kind` is `public-access-proof`,
  and `substrate.kind` is `public-github`.
- `GH_TOKEN`, `GITHUB_TOKEN`, `gh_auth_present`, and
  `authorization_header_used` are all false.
- `/releases/latest/download/install.sh` and
  `/releases/download/<tag>/install.sh` resolve to the same stable tag and have
  the same SHA256 digest.
- the public release contains the complete installer asset set, including
  `install.sh`, the asset index, build manifest, integrity predicate,
  attestation bundle, attestation metadata, and the default Linux tarball.
  The release may also contain the post-publication
  `m80-latest-freshness-proof.json` evidence asset.
- every downloaded asset records HTTP status, final URL, size, and SHA256.
- integrity subjects, build metadata, release tag, repository, and source
  commit agree with the release being promoted.
- the receipt carries the command, exit status, stdout, stderr, resolved tag,
  and substrate fields required by verified-close policy.

Fixture receipts are useful for verifier tests, but they do not satisfy the
real lane. A latest promotion is not complete until a non-fixture receipt has
been generated from the public `moradology/m80` GitHub release after GitHub
latest resolves to the promoted tag.
