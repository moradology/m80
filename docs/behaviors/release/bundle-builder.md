# Release Bundle Builder

`scripts/package-release-bundle.py` is the release-packaging entrypoint for the
default Linux x86_64 minimal product. It does not build artifacts itself. Every
payload path is supplied explicitly on the command line, then the script writes a
deterministic dist directory.

Required inputs:

- `--release-tag`, matching `v<workspace package version>`;
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
install.sh
install.sh.sha256
SHA256SUMS
```

The tarball contains the contract paths from
[`bundle-contract.md`](bundle-contract.md). `bundle.json` is also copied beside
the tarball as `m80-linux-x86_64.bundle.json` so release tooling can inspect
metadata before extraction. `m80-release-assets.json` is generated from the
dist files and names the default host tuple, bundle digest, metadata digest,
schema versions, guest protocol, and Firecracker version. The public
`SHA256SUMS` covers the tarball, installer, metadata sidecar, and asset index.
The public `install.sh` and the bundled `install.sh` are the same rendered
versioned installer asset. The renderer fills in the concrete release tag and
bundle URL, and rejects templates that call `m80 quickstart`, mention
`scripts/quickstart.sh`, or use artifact-only `--artifact-url`.

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
assets, wrong tuple, wrong hash, duplicate tuple, stale version, and a missing
index checksum sidecar.

`scripts/verify-release-integrity.py` validates the release-integrity predicate
used by the signing/attestation lane. That predicate is documented in
[`release-integrity-material.md`](release-integrity-material.md) and records the
release tag, commit SHA, target, Rust toolchain, m80 package version, bundle
metadata hash, and every current public dist asset digest. The verifier also
loads the anchored trust policy, `m80-release-integrity.attestation.jsonl`, and
`m80-release-attestation.json` so human verification and installer verification
share the same cryptographic GitHub attestation, signer, keyset, expiry, and
rotation checks.
