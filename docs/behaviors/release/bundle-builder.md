# Release Bundle Builder

`scripts/package-release-bundle.py` is the release-packaging entrypoint for the
default Linux x86_64 minimal product. It does not build artifacts itself. Every
payload path is supplied explicitly on the command line, then the script writes a
deterministic dist directory.

Required inputs:

- `--release-tag`, matching `v<workspace package version>`;
- `--commit-sha`, the 40-character source commit SHA attested for the
  release;
- `--rust-toolchain`, the Rust toolchain used to build the release binaries;
- one or more `--target-triple` values, including
  `x86_64-unknown-linux-musl` for the guest daemon;
- `--builder-identity` and `--builder-os-image`;
- either one or more `--apt-package-version` `PACKAGE=VERSION` rows or
  `--container-digest`;
- `--target linux-x86_64` and `--image-kind minimal` for the supported default
  product;
- `--m80-bin`, built with matching `M80_RELEASE_TAG`;
- `--jailer-harden-bin` and `--net-helper-bin`;
- `--kernel`, `--rootfs`, `--rootfs-manifest`, and `--build-receipt`;
- `--guestd`;
- `--install-sh`, the versioned installer template, normally
  `scripts/install.sh`;
- `--out-dir`.

The dist directory contains:

```text
m80-linux-x86_64.tar.gz
m80-linux-x86_64.tar.gz.sha256
m80-linux-x86_64.bundle.json
m80-linux-x86_64.bundle.json.sha256
m80-release-assets.json
m80-release-assets.json.sha256
m80-bootstrap-selector.tsv
m80-bootstrap-selector.tsv.sha256
m80-release-build.json
m80-release-build.json.sha256
install.sh
install.sh.sha256
m80-release-integrity.json
SHA256SUMS
```

The tarball contains the contract paths from
[`bundle-contract.md`](bundle-contract.md). `bundle.json` is also copied beside
the tarball as `m80-linux-x86_64.bundle.json` so release tooling can inspect
metadata before extraction. `m80-release-assets.json` is generated from the
dist files and names the default host tuple, bundle digest, metadata digest,
schema versions, guest protocol, and Firecracker version.
`m80-bootstrap-selector.tsv` is generated from that index for the POSIX
bootstrap path that runs before a local `m80` binary exists. The public
`m80-release-build.json` records the source commit, Rust toolchain, target
triples, `Cargo.lock` digest, builder identity, builder OS image, and builder
package versions or container digest. The public `SHA256SUMS` covers the
tarball, installer, metadata sidecar, asset index, bootstrap selector, and
build manifest.
The public `install.sh` and the bundled `install.sh` are the same rendered
versioned installer asset. The renderer fills in only the concrete release tag;
bundle selection comes from the verified bootstrap selector and canonical asset
index downloaded from that pinned release. The renderer rejects templates that
carry public-path `M80_BUNDLE_URL` or `M80_BUNDLE_NAME` placeholders, hardcode a
tuple-specific artifact name, call `m80 quickstart`, mention
`scripts/quickstart.sh`, use artifact-only `--artifact-url`, or introduce any
new `@M80_*@` placeholder. The installer template is POSIX `/bin/sh` code; CI
runs `sh -n scripts/install.sh` and `shellcheck -s sh scripts/install.sh` so the
published `curl | sudo sh` path does not depend on bash-only syntax.

The package command also emits `m80-release-integrity.json`. The tag workflow
attests that predicate, then appends `m80-release-integrity.attestation.jsonl`
and `m80-release-attestation.json` before uploading the release dist artifact.

## Determinism

The tarball writer uses sorted paths, fixed owner/group IDs, fixed mtimes, and
fixed modes:

- executables: `bin/m80`, `bin/m80-jailer-harden`, `bin/m80-net-helper`,
  `install.sh` use `0755`;
- artifact payloads, metadata, and checksum files use `0644`.

Re-running the package command with identical inputs must produce the same
tarball sha256.

## Compatibility Gate

Packaging is the pre-upload compatibility gate. Before any bundle is written,
the builder:

- runs `m80 --json version` and requires release identity to match
  `--release-tag`;
- reads the schema versions reported by the m80 binary and requires the guest
  manifest and build receipt to use those exact schemas;
- runs `m80-guestd --version` and requires its package version and protocol
  version to match the host binary tuple;
- reads the guest manifest and build receipt, then verifies the manifest hash,
  build-receipt manifest path, kernel hash, rootfs hash, and guestd hash
  against the supplied files;
- requires the supported OS/arch tuple (`linux`, `x86_64`), image kind
  (`minimal`), Firecracker version pin, and install-provenance requirement to
  be recorded in `bundle.json`.

The gate rejects stale binaries, protocol drift, schema drift, wrong image
kind, wrong target architecture, receipt/manifest hash mismatch, and any
bundle metadata that omits the install-provenance requirement.

## Verification

`scripts/verify-release-bundle.py --verify-sidecars` validates the tarball and
adjacent dist sidecars. Regression coverage in `scripts/test-release-bundle.py`
checks successful packaging plus rejection of missing required paths, duplicate
paths, unexpected paths, wrong modes, stale versions, metadata hash mismatches,
schema/protocol/receipt mismatches, missing install-provenance metadata, and
stale public checksum sidecars. It also checks the asset-index path for missing
assets, wrong tuple, wrong hash, duplicate tuple, stale version, a missing index
checksum sidecar, bootstrap-selector drift from the JSON index, and build
manifest drift from the bundle metadata, source commit, `Cargo.lock`, or
builder-material contract.

`scripts/verify-release-integrity.py` validates the release-integrity predicate
used by the signing/attestation lane. That predicate is documented in
[`release-integrity-material.md`](release-integrity-material.md) and records the
release tag, commit SHA, target, Rust toolchain, m80 package version, bundle
metadata hash, build-manifest subjects, and every current public
installer/bootstrapper-consumed dist asset digest. The verifier also
loads the anchored trust policy, `m80-release-integrity.attestation.jsonl`, and
`m80-release-attestation.json` so human verification and installer verification
share the same cryptographic GitHub attestation, signer, keyset, expiry, and
rotation checks.
