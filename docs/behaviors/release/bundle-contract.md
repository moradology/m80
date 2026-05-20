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
Firecracker, jailer, and the Firecracker seccomp filter are known. That
installed manifest records the seccomp filter under `launch_material`, not as a
host binary.

The bundle also must not carry operator-provided host prerequisite payloads.
For v0.x, names such as `bin/firecracker`, `bin/jailer`, and
`bin/firecracker-seccomp-filter.bin` are rejected rather than treated as
optional convenience files. m80 owns its own CLI/helper binaries and guest
artifacts; the official Firecracker train remains an operator-provided host
prerequisite.

## Metadata

`bundle.json` uses `schema_version: 1` and records:

- `release_tag` and `m80_version`;
- `package_version`;
- `target`, currently `linux-x86_64`;
- `os` and `arch`, currently `linux` and `x86_64`;
- `image_kind`, currently `minimal` for the default quickstart;
- `m80_protocol_version`;
- `guestd_package_version`;
- `guest_protocol_version`;
- `manifest_schema_version`;
- `build_receipt_schema_version`;
- `build_receipt_manifest_path`;
- `install_provenance_schema_version`;
- `install_provenance_required: true`;
- `expected_firecracker_version`;
- one sha256 and size entry for each payload path.

The metadata file set is the payload set: `bin/*`, `artifacts/*`, and
`install.sh`. `SHA256SUMS` additionally covers `bundle.json` so the installed
metadata bytes are checked before use.

The release dist directory also publishes `m80-linux-x86_64.bundle.json` as a
byte-identical metadata sidecar, checksum sidecars for public assets, and a
public `SHA256SUMS` covering the tarball, installer, and metadata sidecar. See
[`bundle-builder.md`](bundle-builder.md).

## Public Release Subject Set

Signed release integrity material must cover every public dist asset consumed
by the installer or bootstrapper. For the default Linux bundle, the complete
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

There are no detached signature files in v1. If the asset index later names a
signature or proof file that the installer consumes, the signed/attested
predicate must add that file as a subject in the same release.

Guest manifest and build-receipt bytes are release payloads. Installers must not
rewrite them silently after bundle verification. If install-time relocation is
needed, it must produce a separate installed-provenance record that names the
original hash, installed hash, path rewrite, and release tag. See
[`install-provenance.md`](install-provenance.md).

## Verification

`scripts/verify-release-bundle.py` rejects:

- missing required paths;
- duplicate tar entries;
- unexpected tar entries;
- install-time-only host-binaries manifests inside the bundle;
- operator-provided Firecracker, jailer, or Firecracker seccomp filter payloads
  inside the bundle;
- wrong file modes for required paths;
- target, OS, arch, or image-kind mismatch;
- stale release tag, m80 version, or package version;
- missing manifest/protocol/receipt/provenance/Firecracker metadata;
- manifest, build-receipt, or bundle compatibility tuple mismatch;
- metadata or `SHA256SUMS` hash mismatch;
- stale adjacent dist checksum sidecars when `--verify-sidecars` is set.

`scripts/package-release-bundle.py` creates this shape from already-built
inputs with deterministic tar metadata and refuses a binary whose
`m80 --json version` identity does not match the release tag. It also refuses
guestd protocol drift, manifest schema drift, build-receipt schema drift, wrong
image kind, wrong target architecture, and manifest/receipt hashes that do not
match the supplied release artifacts.
