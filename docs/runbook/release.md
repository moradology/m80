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
  --install-sh scripts/quickstart.sh \
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

The installer bead will replace `scripts/quickstart.sh` with the final
versioned `install.sh` release asset; the bundle contract already reserves the
in-bundle path as `install.sh`.

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
