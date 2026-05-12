# `m80-image-manifest`

The schema, validator, and sha256 verifier for `<rootfs>.manifest.json`
and `<rootfs>.manifest.json.sha256` — the provenance record that travels
beside every m80 guest image.

## Reason for being

Single source of truth shared by `m80-image-build` (writer) and
`m80-preflight` (reader/verifier) so the schema cannot drift. Validation
logic (sha256 recompute, schema-version probe) lives here once.

## Black-box contract

- `schema_version` is the integer **4**. `read` rejects any other value with
  `ManifestError::UnsupportedSchemaVersion` before unknown-field detection,
  so an unknown future version is reported as a version error, not a parse
  error. `write` enforces the same invariant before creating the file. There
  is no migration path inside this crate; new schema versions are new code.
- `image_kind: ImageKind` declares which userland family the image was
  built for: `Ubuntu` (Ubuntu userland from the Firecracker CI squashfs)
  or `Minimal` (busybox userland built from scratch). Both kinds boot
  m80-guestd as PID 1 and share the same overlay/workspace drive contract.
- `kernel_kind: KernelKind` declares which kernel was used: `Stock`
  (upstream Firecracker CI kernel from S3) or `Stripped` (purpose-built
  via the m80-ci9i.2 pipeline). Defaults to `Stock` when absent in JSON.
- The manifest covers up to four hash-bearing artifacts. `kernel_image`,
  `output_rootfs_image`, and `daemon_binary_path` are always present;
  `source_rootfs_image` is `Option<>` and present iff
  `image_kind == Ubuntu`. The kind/field invariant is enforced on `read`,
  `write`, and `verify` and surfaces as `ManifestError::InconsistentKind`.
- `no_egress_reason` is operator audit metadata. m80-built images use
  `DEFAULT_NO_EGRESS_REASON` to state that the image is network-neutral and
  outbound access requires an explicit runtime policy.
- `verify(&Manifest, root: &Path) -> Result<(), ManifestError>` recomputes
  every populated sha256 from the on-disk artifact and compares to the
  recorded value. Fields that are `None` for the manifest's kind are
  skipped. Any mismatch is fatal; there is no "warn and continue."
- `expected_firecracker_version` is recorded but **not** enforced inside
  this crate; the live-binary version comparison is the caller's job (e.g.,
  `m80-preflight`). This crate has no `FirecrackerVersionMismatch` variant.
- The manifest is **side-by-side** with the rootfs (`<rootfs>.manifest.json`).
  This crate does not look up a manifest by some registry or env var.

## Schema

### v4 (current)

`schema_version: 4`. Removed the systemd service and workspace-mount unit
artifact fields. Both image kinds now boot m80-guestd as PID 1; `Ubuntu`
records only its source-rootfs provenance in addition to common artifacts.
Existing v3 manifests must be rebuilt.

### v3

`schema_version: 3`. Added `kernel_kind: KernelKind` field (§ Public
surface). The field has `#[serde(default)]` so JSON without `kernel_kind`
deserializes as `KernelKind::Stock`. Existing v2 manifests must be rebuilt
(no migration code — per CLAUDE.md, new schema versions are new code).

### v2

Added `image_kind` discriminator and made systemd-related fields `Option<>`
so a `Minimal` image can emit a manifest without lying about absent
artifacts.

## Public surface

- `Manifest` — struct mirroring the JSON. All fields are public:
  `daemon_binary_path`, `daemon_binary_sha256`,
  `expected_firecracker_version`, `guest_port`, `image_kind`,
  `kernel_image`, `kernel_image_sha256`, `kernel_kind`,
  `no_egress_reason`, `output_rootfs_image`, `output_rootfs_sha256`,
  `ready_marker`, `schema_version`, `source_rootfs_image`, and
  `source_rootfs_sha256`. Paired `<artifact>_path` / `<artifact>_sha256`
  fields cover each hash-bearing artifact. The Ubuntu-only source-rootfs
  fields are `Option<>`. Field declaration order is alphabetical.
- `ImageKind { Ubuntu, Minimal }` — discriminator on `Manifest`.
- `KernelKind { Stock, Stripped }` — kernel provenance discriminator.
  `Default = Stock`. Serializes as `"stock"` / `"stripped"`.
- `Manifest::read(path: &Path) -> Result<Manifest, ManifestError>` — peek
  `schema_version` first via a probe struct, then deserialize the full
  struct, then enforce the kind/field invariant.
- `Manifest::write(&self, path: &Path) -> Result<(), ManifestError>` —
  enforces `schema_version == SCHEMA_VERSION` and the kind/field invariant
  before writing. Caller is responsible for the parent directory existing.
- `Manifest::verify(&self, root: &Path) -> Result<(), ManifestError>` —
  recompute sha256 for every populated artifact and compare; skip
  `None`-valued fields.
- `SCHEMA_VERSION: u32 = 4`.
- `DEFAULT_NO_EGRESS_REASON: &str` — default human-readable audit string for
  m80-built network-neutral images.
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
- Mutating any current sha256 field by one byte and calling `verify`
  returns `Sha256Mismatch` naming that field.
- Setting an older `schema_version` returns `UnsupportedSchemaVersion`, even
  when the same JSON also carries an unknown field — the version check
  fires first.
- Pointing one of the artifact paths at a nonexistent file and calling
  `verify` returns `Io { path, source: NotFound }` carrying the missing
  path.
- Schema v3: `kernel_kind` absent from JSON deserializes as `KernelKind::Stock`.
- Schema v3: `KernelKind::Stripped` roundtrips through write → read.
- `KernelKind::default()` is `Stock` (Rust Default trait check).
