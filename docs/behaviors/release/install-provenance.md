# Install Provenance

`install-provenance.json` is an install-time receipt for the only guest-artifact
mutation the release installer may perform: rewriting verified guest manifest
and build-receipt bytes so their paths point at final installed host paths.

The release bundle keeps `output.ext4.manifest.json` and
`output.ext4.build-receipt.json` as immutable payloads. After the bundle hash is
verified, an installer that rewrites those JSON files must write
`install-provenance.json` beside the installed rootfs. The record uses
`schema_version: 1` and contains:

- `release_tag`, the concrete release tag when the installer knows it;
- one `guest_manifest` transform;
- one `build_receipt` transform;
- for each transform, source path, source sha256, installed path, installed
  sha256, and `rewrite: "install_path_rewrite"`.

Legacy artifact-only quickstart installs may have `release_tag: null` because
they do not resolve a concrete tag. The release-bundle layout installer records
the bundle's concrete release tag.

## Verification

`m80 quickstart --no-run` writes `install-provenance.json` after relocating the
manifest and build receipt. Preflight treats the file as authoritative when it
is present: it must contain exactly one transform for the manifest and one for
the build receipt, each transform's installed path must match the file preflight
is about to trust, and each installed sha256 must match the current bytes.

Build outputs that were never relocated do not need this file. Installed
release artifacts that rewrite verified bundle payloads do. Preflight treats
`host-binaries.manifest.json` beside the rootfs as the install-time marker; when
that marker exists, `install-provenance.json` must also exist and verify.

## Regression Coverage

- `crates/m80-cli/tests/quickstart_smoke.rs::quickstart_no_run_installs_verified_artifacts`
  proves quickstart emits both transforms and that their installed hashes match
  the installed files.
- `crates/m80-cli/tests/release/installer_layout.rs::install_bundle_layout_copies_verified_bundle_into_version_dir`
  proves the release-bundle layout installer records the concrete release tag
  and the installed manifest/receipt rewrites.
- `crates/m80-preflight/src/artifacts/tests.rs::install_provenance_covering_rewritten_manifest_and_receipt_passes`
  proves preflight accepts a valid installed provenance chain.
- `crates/m80-preflight/src/artifacts/tests.rs::install_provenance_missing_for_installed_artifacts_fails_preflight`
  proves installed artifacts cannot omit the provenance record.
- `crates/m80-preflight/src/artifacts/tests.rs::install_provenance_manifest_hash_mismatch_fails_preflight`
  proves changed installed bytes cannot pass with stale provenance.
- `crates/m80-preflight/src/artifacts/tests.rs::install_provenance_missing_receipt_transform_fails_preflight`
  proves a partial provenance record fails closed.
