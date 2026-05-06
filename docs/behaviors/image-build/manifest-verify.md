# Image Build — Manifest Verification

Source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/foundation.rs`
and `src/storage.rs`; m80 crate `m80-image-manifest`.

## schema-check {#schema-check}

`Manifest::read(path)` rejects malformed or incompatible manifests before the
caller can use them:

- Missing required fields return `ManifestError::Json(_)`.
- Unknown fields return `ManifestError::Json(_)` through
  `#[serde(deny_unknown_fields)]`.
- `schema_version != SCHEMA_VERSION` returns
  `ManifestError::UnsupportedSchemaVersion(v)`.
- The schema-version probe runs before unknown-field rejection, so a future
  manifest with extra fields still reports the unsupported version cleanly.
- `ImageKind` invariants are enforced: Ubuntu manifests must include
  systemd/source-rootfs fields, Minimal manifests must omit them.

There is no partial acceptance and no migration path.

Tests: `m80-image-manifest/tests/manifest_verify_schema.rs` and
`m80-image-manifest/tests/manifest_kind_invariants.rs`.

## sha256-recompute {#sha256-recompute}

`Manifest::verify(root)` recomputes sha256 over every populated artifact path:
kernel, source rootfs, output rootfs, daemon binary, service unit, and
workspace mount unit. Absolute paths are used as-is; relative paths are joined
with `root`. Minimal manifests skip the fields that are `None`.

Each artifact is streamed through SHA-256 in 64 KiB chunks. The recomputed
lowercase hex digest must exactly match the recorded digest. Any mismatch
returns `ManifestError::Sha256Mismatch { field, expected, actual }` naming the
path field that failed. Missing files return `ManifestError::Io { path,
source: NotFound }`. There is no warning mode.

Tests:
`m80-image-manifest/tests/manifest_sha256_coverage.rs::sha256_covers_all_inputs`,
`m80-image-manifest/tests/manifest_verify_missing.rs`, and
`m80-image-build/tests/verify_with_fixture_manifest.rs::verify_fails_on_tampered_artifact`.
