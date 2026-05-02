# `m80-image-manifest`

The schema, validator, and sha256 verifier for `<rootfs>.manifest.json`
and `<rootfs>.manifest.json.sha256` — the provenance record that travels
beside every m80 guest image.

## Reason for being

The manifest is written **at build time** by `m80-image-build` and read
**at boot time** by `m80-preflight`. If the two ends drift on what fields
exist, what `schema_version` means, or how the sha256 over multiple files
is computed, boot silently accepts a tampered or stale image.

`m80-image-manifest` is the one crate both ends depend on. There is no
"writer's view" and "reader's view" — there's a struct, and it's the
contract.

A second reason: validation logic (sha256 recompute, schema-version check,
expected-firecracker-version match) is non-trivial and worth testing once,
not twice.

## Black-box contract

- `schema_version` is the integer **1** for v0.1. A manifest with any other
  value rejects with `ManifestError::UnsupportedSchemaVersion`. The
  schema-version check fires **before** unknown-field detection, so an
  unknown future version is reported as a version error, not a parse error.
  There is no migration path inside this crate; new schema versions are
  new code.
- The manifest covers exactly six hash-bearing artifacts, each with its own
  path + sha256 fields: kernel image, source rootfs, output rootfs, m80
  daemon binary, systemd service unit, and systemd workspace mount unit.
  Adding a seventh requires a `schema_version` bump.
- `verify(&Manifest, root: &Path) -> Result<(), ManifestError>` recomputes
  every sha256 from the on-disk artifact and compares to the recorded
  value, covering all six artifacts in one call. Any mismatch is fatal;
  there is no "warn and continue."
- `expected_firecracker_version` is recorded but **not** enforced inside
  this crate; the live-binary version comparison is the caller's job (e.g.,
  `m80-preflight`). This crate has no `FirecrackerVersionMismatch` variant.
- The manifest is **side-by-side** with the rootfs (`<rootfs>.manifest.json`).
  This crate does not look up a manifest by some registry or env var.
- Output is pretty-printed JSON with a trailing `\n`. Field order is
  alphabetical because the struct fields are declared alphabetically;
  `read → write` is byte-identical so long as that order doesn't change.
  No separate canonicalization pass.

## Public surface

- `Manifest` — struct mirroring the JSON. All fields public, including
  paired `<artifact>_path` / `<artifact>_sha256` fields for each of the
  six hash-bearing artifacts. Field declaration order is alphabetical.
- `Manifest::read(path: &Path) -> Result<Manifest, ManifestError>` — peek
  `schema_version` first via a probe struct (so a future schema version
  reports `UnsupportedSchemaVersion` rather than an unknown-field error),
  then deserialize the full struct.
- `Manifest::write(&self, path: &Path) -> Result<(), ManifestError>` —
  caller is responsible for the parent directory existing.
- `Manifest::verify(&self, root: &Path) -> Result<(), ManifestError>` —
  recompute sha256 for all six artifacts and compare.
- `SCHEMA_VERSION: u32 = 1`.
- `ManifestError`: `UnsupportedSchemaVersion(u32)`,
  `Sha256Mismatch { field, expected, actual }`,
  `Io { path, source }`, `Json(serde_json::Error)`. The `Io` variant
  carries the path the I/O failed on, so a missing artifact surfaces as
  `Io { path, source: NotFound }` without a separate variant.

## Non-goals

- **No image discovery.** `m80-image-manifest` doesn't search for a
  manifest. The caller (preflight or build tool) hands it a path.
- **No interpreter inventory.** The predecessor manifest carries a
  `guest_runtime` block listing Python/Node versions; m80 drops that
  surface (interpreter packaging is a consumer concern).
- **No signature verification.** sha256 is integrity, not authenticity.
  Signing is layered on top by whoever distributes the image.

## Dependencies

- `serde`, `serde_json`, `sha2`, `hex`.
- `thiserror`.
- (no other m80 crates — this is a leaf.)

## Tests

- A golden manifest fixture round-trips byte-equivalent through
  `read` → `write`.
- Mutating any of the six sha256 fields by one byte and calling `verify`
  returns `Sha256Mismatch` naming that field.
- Setting `schema_version: 2` returns `UnsupportedSchemaVersion(2)`, even
  when the same JSON also carries an unknown field — the version check
  fires first.
- Pointing one of the six artifact paths at a nonexistent file and calling
  `verify` returns `Io { path, source: NotFound }` carrying the missing
  path.
