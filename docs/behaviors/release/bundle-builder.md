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
- `--install-sh`;
- `--out-dir`.

The dist directory contains:

```text
m80-linux-x86_64.tar.gz
m80-linux-x86_64.tar.gz.sha256
m80-linux-x86_64.bundle.json
m80-linux-x86_64.bundle.json.sha256
install.sh
install.sh.sha256
SHA256SUMS
```

The tarball contains the contract paths from
[`bundle-contract.md`](bundle-contract.md). `bundle.json` is also copied beside
the tarball as `m80-linux-x86_64.bundle.json` so release tooling can inspect
metadata before extraction. The public `SHA256SUMS` covers the tarball,
installer, and metadata sidecar.

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
stale public checksum sidecars.
