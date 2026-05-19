# Release Bundle Contract

The Linux release bundle is the product shape that hides the internal
binary/artifact split from normal users. A valid bundle is one tarball named
`m80-linux-x86_64.tar.gz` with these paths:

```text
bin/m80
bin/m80-jailer-harden
bin/m80-net-helper
artifacts/vmlinux
artifacts/output.ext4
artifacts/output.ext4.manifest.json
artifacts/output.ext4.build-receipt.json
artifacts/m80-guestd
install.sh
bundle.json
SHA256SUMS
```

`artifacts/host-binaries.manifest.json` is not a bundled file. It is generated
by the installer after the final host paths for m80, m80 helpers,
Firecracker, jailer, and the Firecracker seccomp filter are known.

## Metadata

`bundle.json` uses `schema_version: 1` and records:

- `release_tag` and `m80_version`;
- `package_version`;
- `target`, currently `linux-x86_64`;
- `image_kind`, currently `minimal` for the default quickstart;
- `guest_protocol_version`;
- `manifest_schema_version`;
- `expected_firecracker_version`;
- one sha256 and size entry for each payload path.

The metadata file set is the payload set: `bin/*`, `artifacts/*`, and
`install.sh`. `SHA256SUMS` additionally covers `bundle.json` so the installed
metadata bytes are checked before use.

Guest manifest and build-receipt bytes are release payloads. Installers must not
rewrite them silently after bundle verification. If install-time relocation is
needed, it must produce a separate installed-provenance record that names the
original hash, installed hash, path rewrite, and release tag.

## Verification

`scripts/verify-release-bundle.py` rejects:

- missing required paths;
- duplicate tar entries;
- install-time-only host-binaries manifests inside the bundle;
- target or image-kind mismatch;
- stale release tag, m80 version, or package version;
- missing manifest/protocol/Firecracker metadata;
- metadata or `SHA256SUMS` hash mismatch.

`scripts/package-release-bundle.py` creates this shape from already-built
inputs and refuses a binary whose `m80 --json version` identity does not match
the release tag.
