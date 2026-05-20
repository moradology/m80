# Release Runbook

This runbook owns the release identity contract used by the installer and
quickstart flow.

## Version Source

Release builds inject the GitHub release tag at compile time:

```sh
M80_RELEASE_TAG=vX.Y.Z cargo build --release -p m80-cli
```

The injected tag must be the exact `v<workspace package version>` tag. For the
workspace package version `0.0.0`, the expected release tag is `v0.0.0`.

`m80 --version` and `m80 version` expose the release identity:

- dev builds render as `<package-version>-dev`;
- release builds render the injected GitHub release tag;
- `m80 --json version` includes `package_version`, `release_tag`,
  `release_build`, `version_status`, `expected_release_tag`,
  `protocol_version`, `manifest_schema_version`,
  `build_receipt_schema_version`, and
  `install_provenance_schema_version`.

Packaging must refuse to publish a bundle when `version_status` is `dev` or
`mismatch`. The installer and quickstart resolver must not use `releases/latest`
from a dev build; dev builds require an explicit local bundle or artifact URL.

## Verification

The release identity is pinned by:

- `crates/m80-cli/src/release.rs` unit tests for dev, release, and mismatch
  identity;
- `crates/m80-cli/tests/version_smoke.rs` for `m80 --version` and JSON
  `m80 version` output.

## Bundle Contract

The Linux bundle contract is documented in
`docs/behaviors/release/bundle-contract.md`. Build the bundle from already-built
inputs with:

```sh
scripts/package-release-bundle.py \
  --release-tag "$M80_RELEASE_TAG" \
  --target linux-x86_64 \
  --image-kind minimal \
  --m80-bin target/release/m80 \
  --jailer-harden-bin target/release/m80-jailer-harden \
  --net-helper-bin target/release/m80-net-helper \
  --kernel /tmp/m80-release-artifacts/vmlinux \
  --rootfs /tmp/m80-release-artifacts/output.ext4 \
  --rootfs-manifest /tmp/m80-release-artifacts/output.ext4.manifest.json \
  --build-receipt /tmp/m80-release-artifacts/output.ext4.build-receipt.json \
  --guestd /tmp/m80-release-artifacts/m80-guestd \
  --install-sh scripts/install.sh \
  --out-dir /tmp/m80-release-bundle
```

This package command is the compatibility gate before upload. It reads
`m80 --json version`, `m80-guestd --version`, the guest manifest, and the build
receipt; then it refuses schema drift, protocol drift, wrong target/image kind,
stale release identity, or manifest/receipt hashes that do not match the
supplied artifacts.

Do not add official Firecracker, official jailer, or Firecracker seccomp filter
payloads to the bundle. v0.x policy treats those bytes as operator-provided host
prerequisites, so both the bundle verifier and `m80 quickstart` reject them
before an install can become active. If m80 ever starts installing a pinned
Firecracker train itself, file a new policy decision and hard-cutover bead
instead of making the current verifier tolerant.

`scripts/install.sh` is the versioned installer template. Packaging renders it
with the concrete release tag and bundle URL, publishes it as `install.sh`, and
embeds the same rendered file inside the release bundle. It downloads the
matching bundle, verifies the adjacent checksum sidecar, extracts that bundle's
`bin/m80`, then hands off to `m80 install --bundle-url file://...`. It must not
call `scripts/quickstart.sh` or the legacy artifact-only quickstart flow.

Verify a produced bundle before upload:

```sh
scripts/verify-release-bundle.py \
  /tmp/m80-release-bundle/m80-linux-x86_64.tar.gz \
  --release-tag "$M80_RELEASE_TAG" \
  --verify-sidecars
```

The package step emits a deterministic tarball, checksum sidecars, an
inspectable `m80-linux-x86_64.bundle.json` metadata sidecar, and a public
`SHA256SUMS` for the tarball, installer, and metadata sidecar. The exact builder
contract is captured in `docs/behaviors/release/bundle-builder.md`.

The complete public release subject set for the signed/attested default Linux
dist is:

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

## Asset Index

The release asset index is the machine-readable selector for public bundles.
It lists every bundle by OS, architecture, image kind, release tag, m80 version,
guest protocol, manifest schema, expected Firecracker version, tarball digest,
metadata digest, and integrity-material references. The default Linux
quickstart tuple is `linux` / `x86_64` / `minimal`. The published file is
`m80-release-assets.json`; its checksum sidecar and the public `SHA256SUMS`
cover the index before the publish job re-downloads and validates it. The
current publisher leaves signature/attestation reference fields nullable until
the release signing lane defines and emits those proof assets.

## Release Integrity Material

The release signing lane uses the schema in
`docs/behaviors/release/release-integrity-material.md`. The public mechanism is
GitHub Artifact Attestations over `m80-release-integrity.json`; that predicate
records the release tag, commit SHA, target, Rust toolchain, m80 package
version, bundle metadata hash, and the sha256/size of every current public
dist asset. The same verifier also loads
`docs/behaviors/release/m80-release-trust-policy.json`,
`m80-release-integrity.attestation.jsonl`, and
`m80-release-attestation.json` so human verification and installer verification
share one trust-anchor path. The trust check uses `gh attestation verify`; use a
GitHub CLI build with `gh attestation` support.

Before wiring signed material into installers, verify the predicate shape
against a downloaded dist directory. This command requires the signing lane to
publish the attestation bundle and normalized metadata; a release that lacks
those files is checksum-only and must not be treated as signed. Run it from a
trusted m80 checkout or installed verifier distribution; do not load the trust
policy from the release dist being verified:

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

This check is read-only and does not require root. It fails closed for wrong
tag, wrong commit, missing subject digests, unknown signers, stale keysets,
expired trust material, unsigned/downgraded material, tampered bundle or
installer bytes, failed GitHub attestation verification, unsupported schema,
and bundle metadata or asset-index tag drift.

To add a new architecture or image kind, add a new asset-index row and publish
the matching bundle, metadata sidecar, checksums, and integrity material. The
README command stays the same: the installer/bootstrapper reads the verified
index and selects the matching host tuple without changing README commands. Do
not add architecture-specific README commands unless the common installer cannot
select the tuple.

The workflow stages and token boundary for building and publishing release
artifacts are recorded in `docs/runbook/release-bundle.md`. The short version:
build jobs run with `contents: read`; the tag-only publish job is the only stage
with `contents: write`; and `scripts/lint-github-workflows.py` keeps that
boundary from drifting in CI.

## Host Train Proof

Before promoting a release on a target host, save a preflight proof:

```sh
m80 preflight --json > release-preflight-proof.json
```

The proof is a `HostPrerequisiteResult`. It must contain a
`Firecracker binary` check whose expected and observed version fields record
the Firecracker version, and a `Jailer binary` check whose expected and observed
version fields record the jailer version. The train policy source is
`crates/m80-preflight/src/firecracker_train.rs`; the CVE-floor table source is
`crates/m80-preflight/src/cve_floor.rs`.
