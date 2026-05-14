# Build Receipt Integrity

Every m80 image build emits `<rootfs>.build-receipt.json` after
`<rootfs>.manifest.json` is written. The receipt records the sha256 of the
manifest bytes and the artifact path/hash tuples for the kernel, output rootfs,
daemon binary, and Ubuntu source rootfs when present.

`m80-preflight` reads the receipt beside the selected rootfs. It rejects a
missing receipt, a receipt pointing at a different manifest, a manifest sha256
mismatch, duplicate/missing artifact kinds, or any artifact path/hash tuple that
differs from the manifest. The boot-scoped preflight cache does not skip the
receipt check.

The receipt only defends against artifact/manifest co-replacement when deploy
practice gives it separate authority from mutable artifacts: build identity
emits it, deploy identity installs it, and run identity cannot rewrite it.
`docs/ops/host-setup.md` records that operator model.

Evidence:

- `crates/m80-image-manifest/tests/build_receipt.rs`
- `crates/m80-image-build/tests/dry_run_smoke.rs`
- `crates/m80-preflight/src/artifacts.rs::tests::missing_build_receipt_returns_typed_io_error`
- `crates/m80-preflight/src/artifacts.rs::tests::build_receipt_manifest_sha_mismatch_fails_preflight`
- `crates/m80-preflight/src/artifacts.rs::tests::build_receipt_artifact_hash_must_match_manifest`
