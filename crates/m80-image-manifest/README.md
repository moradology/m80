# `m80-image-manifest`

The schema, validator, and sha256 verifier for `<rootfs>.manifest.json`
and `<rootfs>.manifest.json.sha256` — the provenance record that travels
beside every m80 guest image.

## Reason for being

Single source of truth shared by `m80-image-build` (writer) and
`m80-preflight` (reader/verifier) so the schema cannot drift. Validation
logic (sha256 recompute, schema-version probe) lives here once.

## Black-box contract

- `schema_version` is the integer **2**. A manifest with any other value
  rejects with `ManifestError::UnsupportedSchemaVersion`. The schema-
  version check fires **before** unknown-field detection, so an unknown
  future version is reported as a version error, not a parse error.
  There is no migration path inside this crate; new schema versions are
  new code.
- `image_kind: ImageKind` declares which startup model the image was
  built for: `Ubuntu` (systemd as PID 1; m80-guestd is a service) or
  `Minimal` (m80-guestd is PID 1; busybox userland; no systemd).
- The manifest covers up to six hash-bearing artifacts. `kernel_image`,
  `output_rootfs_image`, and `daemon_binary_path` are always present;
  `source_rootfs_image`, `service_unit_path`, and `workspace_mount_path`
  are `Option<>` and present iff `image_kind == Ubuntu`. The
  kind/field invariant is enforced on `read`, `write`, and `verify` and
  surfaces as `ManifestError::InconsistentKind`.
- `verify(&Manifest, root: &Path) -> Result<(), ManifestError>` recomputes
  every populated sha256 from the on-disk artifact and compares to the
  recorded value. Fields that are `None` for the manifest's kind are
  skipped. Any mismatch is fatal; there is no "warn and continue."
- `expected_firecracker_version` is recorded but **not** enforced inside
  this crate; the live-binary version comparison is the caller's job (e.g.,
  `m80-preflight`). This crate has no `FirecrackerVersionMismatch` variant.
- The manifest is **side-by-side** with the rootfs (`<rootfs>.manifest.json`).
  This crate does not look up a manifest by some registry or env var.

## Public surface

- `Manifest` — struct mirroring the JSON. All fields public; paired
  `<artifact>_path` / `<artifact>_sha256` fields for each hash-bearing
  artifact. The five Ubuntu-only fields (`boot_target`,
  `service_unit_*`, `workspace_mount_*`, `source_rootfs_*`) are
  `Option<>`. Field declaration order is alphabetical.
- `ImageKind { Ubuntu, Minimal }` — discriminator on `Manifest`.
- `Manifest::read(path: &Path) -> Result<Manifest, ManifestError>` — peek
  `schema_version` first via a probe struct, then deserialize the full
  struct, then enforce the kind/field invariant.
- `Manifest::write(&self, path: &Path) -> Result<(), ManifestError>` —
  enforces the kind/field invariant before writing. Caller is responsible
  for the parent directory existing.
- `Manifest::verify(&self, root: &Path) -> Result<(), ManifestError>` —
  recompute sha256 for every populated artifact and compare; skip
  `None`-valued fields.
- `SCHEMA_VERSION: u32 = 2`.
- `ManifestError`: `UnsupportedSchemaVersion(u32)`,
  `Sha256Mismatch { field, expected, actual }`,
  `InconsistentKind { kind, field, expected }`,
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
