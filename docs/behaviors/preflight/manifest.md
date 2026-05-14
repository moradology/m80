# Artifact and Manifest Preflight

`m80-preflight` validates the boot artifacts and host paths that Firecracker
launch needs after host capability and binary discovery have passed. The
standalone surface is `verify_artifacts(&ArtifactPreflightConfig)`.

## Kernel Auto-Discovery

When `kernel_image` is not set, m80 lists `vmlinux-*` entries under
`artifact_dir` and selects the lexicographically latest path. With
`ArtifactPreflightConfig::from_env()`, `artifact_dir` comes from
`M80_ARTIFACT_DIR` or defaults to `/opt/m80/artifacts`.

When `kernel_image` is set, it must be an absolute existing host path.

## Rootfs and Manifest

`rootfs_image` is required and must be absolute. The manifest is read from
`<rootfs>.manifest.json`, then `m80-image-manifest` enforces the current manifest
schema and recomputes every recorded sha256. A schema mismatch or digest
mismatch fails closed before any VM launch work begins.

Preflight also reads `<rootfs>.build-receipt.json`. The receipt must point at
the same manifest, its manifest sha256 must match the manifest bytes, and each
receipt artifact path/hash tuple must match the manifest. A missing or
mismatched receipt fails closed before launch.

`M80_KERNEL_KIND=stock|stripped` can override the verified manifest's
`kernel_kind` to match the selected kernel artifact. Other values fail closed.

## Run-Root

`run_root` must be an absolute directory that already exists, has at least 100
MiB free, and is not on a `nodev` mount. m80 does not silently create it during
preflight; the operator or wrapper is responsible for provisioning it.

## Storage Helpers

`mkfs.ext4`, `cp`, `fallocate`, `debugfs`, and `e2fsck` must all resolve on the
configured PATH. The first missing helper returns
`PreflightError::StorageHelperMissing`.

## Evidence

- `crates/m80-preflight/src/artifacts.rs`
- `crates/m80-preflight/src/artifacts.rs::tests::missing_build_receipt_returns_typed_io_error`
- `crates/m80-preflight/src/artifacts.rs::tests::build_receipt_manifest_sha_mismatch_fails_preflight`
- `crates/m80-preflight/src/artifacts.rs::tests::build_receipt_artifact_hash_must_match_manifest`
