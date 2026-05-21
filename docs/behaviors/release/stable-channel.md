# Stable Release Channel

The public convenience install path follows the stable GitHub release channel.
For m80 v0.x, stable means a public GitHub release whose tag is exactly
`vMAJOR.MINOR.PATCH`, with `draft=false`, `prerelease=false`, and the full
installer-consumed asset set present.

Prerelease suffixes such as `v1.2.3-rc.1` are not eligible for the common
`latest` or pinned install paths. A future prerelease lane must add a separate
documented flag and tests in the same diff; the stable path must not silently
accept it.

The stable-channel metadata gate checks:

- `tag_name` is a stable `vMAJOR.MINOR.PATCH` tag;
- `draft` and `prerelease` are both false;
- public release metadata names every installer-consumed asset, including
  `install.sh`, the bundle, checksum sidecars, asset index, bootstrap selector,
  build manifest, release-integrity predicate, attestation bundle, normalized
  attestation metadata, and `SHA256SUMS`;
- each public asset URL points at the configured release repository and the
  same resolved tag;
- the release asset index uses the same `release_tag`, and each indexed bundle
  row reports `m80_version` equal to that tag.

`scripts/stable_release_channel.py` is the reusable verifier for future latest
bootstrap and freshness jobs. `scripts/stable_latest_bootstrap.py` resolves the
GitHub latest release metadata into one stable concrete tag, checks the latest
metadata again before emitting a handoff, and outputs only pinned
`releases/download/<tag>/...` URLs for the installer, bundle, checksum sidecars,
asset index, bootstrap selector, release-integrity predicate, attestation
metadata, and public checksum material. Its URL mode bounds both metadata
fetches with the same connect-timeout, total-timeout, retry, and retry-delay
policy used by installer asset downloads; fixture mode stays network-free.
Current install entry points also enforce the stable tag shape before network or
index work:

- `scripts/install.sh` refuses a rendered non-stable `M80_RELEASE_TAG` before
  release asset downloads;
- `scripts/package-release-bundle.py` refuses to package non-stable release
  tags;
- `m80 install --release-tag` and the hidden bootstrap handoff reject
  prerelease-shaped tags before fetching the asset index;
- direct official `m80 install --bundle-url` GitHub release URLs only accept
  concrete stable bundle assets from `moradology/m80`.

Automatic update and rollback policy uses
`docs/behaviors/release/release-tag-ordering.md` for deterministic ordering
between two stable tags and explicit refusal states for prerelease, build
metadata, malformed, and local/dev identities. Normal install paths already
consume that policy through
`docs/behaviors/release/downgrade-refusal.md`: an older stable target is
refused by default before staging or active-pointer mutation.

Regression coverage:

- `scripts/test-stable-release-channel.py` covers eligible metadata, draft
  release, prerelease release, missing `install.sh`, wrong public asset URL,
  asset-index tag drift, and `m80_version` drift;
- `scripts/test-stable-latest-bootstrap.py` covers latest resolution success,
  tag-switch failure before handoff output, missing metadata, bounded URL-mode
  curl args, HTTP/DNS-or-connect/timeout/malformed-metadata fetch failures, and
  no-network local fixture mode;
- `scripts/test-release-bundle.py::test_rejects_prerelease_release_tag`;
- `crates/m80-cli/src/cmds/install/tests.rs` prerelease rejection tests for
  `--release-tag` and `--bootstrap-tag`.
