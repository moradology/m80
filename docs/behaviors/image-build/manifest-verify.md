# Image provenance manifest — boot-time verification behaviors

Source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/foundation.rs`
(`verify_boot_artifact_manifest`) and `src/storage.rs` (`verify_boot_identity`).
m80 crate: `m80-image-manifest`.

---

## schema-check {#schema-check}

The system rejects boot when the manifest fails schema validation (missing
required fields or incompatible `schema_version`).

**Present-tense statement.** `Manifest::read(path)` deserialises the JSON and
immediately checks `schema_version`:

- If any required field is missing, serde returns a parse error →
  `ManifestError::Json(_)`.
- If an unknown field is present, serde returns a parse error →
  `ManifestError::Json(_)` (enforced by `#[serde(deny_unknown_fields)]` on
  `Manifest`).
- If `schema_version != SCHEMA_VERSION` (currently `1`), the call returns
  `ManifestError::UnsupportedSchemaVersion(v)` where `v` is the value found.

There is no partial-acceptance or migration path inside `m80-image-manifest`;
schema changes are new code and a new `SCHEMA_VERSION` constant.

**predecessor source.**
- `foundation.rs` lines ~727-730: checks `contract.schema_version != 1` and
  returns an error if so.
- `foundation.rs` `verify_boot_artifact_manifest`: calls
  `serde_json::from_slice` then `validate_boot_artifact_manifest`.

**m80 test.** `crates/m80-image-manifest/tests/manifest_verify_schema.rs::rejects_invalid_schema`

---

## sha256-recompute {#sha256-recompute}

The system recomputes the sha256 of the kernel, output rootfs, and daemon at
boot and refuses to boot when any digest differs from the manifest record.

**Present-tense statement.** `Manifest::verify(root)` recomputes sha256 over
the on-disk bytes of all six hash-bearing artifacts (kernel, source rootfs,
output rootfs, daemon binary, systemd service unit, systemd workspace mount
unit) in a single call. For each:

1. The path is resolved: absolute paths are used as-is; relative paths are
   joined with `root`.
2. The file is read in full.
3. sha256 is computed via `sha2::Sha256`.
4. The hex digest is compared to the recorded field value
   (case-insensitive — predecessor uses lowercase; m80 tolerates either case on
   read).
5. On any mismatch, `ManifestError::Sha256Mismatch { field, expected, actual }`
   is returned immediately, naming the manifest path field that failed
   (e.g., `"kernel_image"`, `"service_unit_path"`).
6. On a missing file, `ManifestError::ArtifactMissing(path)` is returned.
7. There is no "warn and continue" — any failure is fatal.

There is no separate `verify_units()` method — the systemd service and
workspace mount units are covered by the single `verify(root)` call alongside
the other four artifacts.

**predecessor source.**
- `storage.rs` `verify_boot_identity` (called from `prepare_vm_storage:42-43`):
  recomputes hashes for kernel, output rootfs, daemon.
- `foundation.rs` `verify_manifest_file_identity`: reads each file and checks
  sha256 + size; mismatch → `FirecrackerError::ArtifactManifestFileTampered`.

**m80 test.** `crates/m80-image-manifest/tests/manifest_verify_sha256.rs::tampered_sha_refused`
