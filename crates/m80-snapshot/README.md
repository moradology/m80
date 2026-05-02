# `m80-snapshot`

Snapshot **manifest schemas** and **persistence path layout** for
Firecracker VM snapshots. Active in v0.1; the actual capture / restore
execution lane is deferred to v0.2.

## Reason for being

The dossier suggested dropping `snapshot.rs` entirely as dead code. A
read of predecessor's contracts (`stage-g-firecracker-snapshot-persistence-contract.md`,
`stage-g-firecracker-snapshot-restore-contract.md`) refuted that: the
manifest schemas and the persistence path template are *required
surfaces* even before the execution lane is wired. A consumer that
writes a snapshot today must use `<store>/<workspace_id>/<run_id>/
<unix_ms>-<sha>/`; if v0.2 changes that, every persisted snapshot
becomes unreadable.

So `m80-snapshot` exists to lock in the schema and path conventions
**now**, even though the capture/restore code itself comes later. The
crate is small but reserves its name and its file format so v0.2 fills
in execution without breaking the on-disk contract.

## Black-box contract

### Persistence path

- Snapshots persist at:
  `<store-root>/<workspace_id>/<run_id>/<created_at_unix_ms>-<artifact_set_sha256>/`.
  This template is the canonical layout. Tools that walk the store rely
  on it.
- `<store-root>` is host-local filesystem only. v0.1 ships no S3/GCS/
  generic-store support. Adding remote stores is a v0.2+ epic.
- A persistence collision (the destination directory already exists)
  fails closed with `SnapshotError::DestinationCollision`. There is no
  silent overwrite. (The collision check is exercised by v0.2 capture;
  the variant is present in v0.1 for callers to match on.)

### Manifest

- `SnapshotManifest` carries five required artifacts: boot identity record,
  memory image, runtime rootfs clone, VM state file, workspace scratch
  image (declared alphabetically by `ArtifactKind` variant). Optional
  artifacts: diagnostics bundle, metrics snapshot.
- `RestoreMetadata` carries the source identity (`source_workspace_id`,
  `source_run_id`, `source_vm_id`), the snapshot path, and the
  expected-Firecracker-version pin. A restore against a different
  Firecracker version fails closed (enforcement is `m80-firecracker`'s
  job at restore time; v0.1 records the value only).
- Restore must materialize into a **fresh VM identity** — the restored
  `vm_id` is required to differ from the source. v0.1 documents this
  invariant; v0.2 enforces it in the execution lane.
- Both files use `#[serde(deny_unknown_fields)]`. An unknown key in a
  v0.2+ file surfaces as `SnapshotError::Json`; a `schema_version`
  mismatch surfaces first as `SnapshotError::UnsupportedSchemaVersion`.

### v0.1 surface

In v0.1 the crate exposes only the schemas + path helpers:

- `SnapshotManifest::write(path: &Path) / read(path: &Path)` — pretty
  JSON + trailing `\n`, mode 0644 on Unix. Parent directory must exist.
- `RestoreMetadata::write / read` — same.
- `persistence_path(store_root, workspace_id, run_id, created_at_ms,
  artifact_set_sha256) -> PathBuf` — pure path construction, no I/O.
- `artifact_set_sha256(&[Artifact]) -> [u8; 32]` — SHA-256 over
  per-artifact JSON bytes concatenated in slice order. The caller
  decides canonical order; this function hashes whatever it receives.
- `SNAPSHOT_MANIFEST_FILE` / `RESTORE_METADATA_FILE` — file-name constants.
- `SCHEMA_VERSION: u32 = 1`.

Capture and restore return `SnapshotError::Deferred` in v0.1.

## Public surface

- `SnapshotManifest` — fields alphabetical: `artifact_set_sha256`,
  `artifacts`, `created_at_unix_ms`, `diagnostics_bundle` (optional),
  `expected_firecracker_version`, `metrics_snapshot` (optional),
  `schema_version`, `source_run_id`, `source_vm_id`, `source_workspace_id`.
- `RestoreMetadata` — fields alphabetical: `expected_firecracker_version`,
  `schema_version`, `snapshot_path`, `source_run_id`, `source_vm_id`,
  `source_workspace_id`.
- `Artifact` — fields alphabetical: `kind`, `path`, `sha256`, `size`.
- `ArtifactKind` — variants alphabetical: `BootIdentity`, `Memory`,
  `RuntimeRootfs`, `VmState`, `WorkspaceScratch`.
- `persistence_path(...)`.
- `artifact_set_sha256(...)`.
- `capture(...)` and `restore(...)` — present in v0.1 but return
  `SnapshotError::Deferred`.
- `SNAPSHOT_MANIFEST_FILE`, `RESTORE_METADATA_FILE`, `SCHEMA_VERSION`.
- `SnapshotError`: `Deferred`, `DestinationCollision`,
  `UnsupportedSchemaVersion(u32)`, `Io { path: PathBuf, source: io::Error }`,
  `Json(serde_json::Error)`.

Note: `FirecrackerVersionMismatch` and `Sha256Mismatch` from the initial
type-pinning are **not** in v0.1 — version enforcement is `m80-firecracker`'s
job at restore time; sha256 verification belongs to the v0.2 execution lane.

## Non-goals

- **No remote store.** Filesystem only.
- **No incremental snapshots.** Full snapshots only.
- **No live migration.** A snapshot is for restart, not for moving a
  running VM.
- **No restore-in-place.** Restore always produces a fresh VM identity.

## Dependencies

- `serde`, `serde_json`, `sha2`, `hex`.
- `thiserror`.
- (no other m80 crates in v0.1; v0.2 will likely depend on
  `m80-firecracker` for capture/restore execution.)

## Tests

Integration tests in `crates/m80-snapshot/tests/`:

- `persistence_path.rs` — exact-output test for `persistence_path()`.
- `artifact_set_sha256.rs` — determinism, sensitivity to per-field and
  order changes.
- `manifest_roundtrip.rs` — `SnapshotManifest::write` then `read`
  produces equal struct; byte-stable round-trip; mode 0644 on Unix;
  alphabetical JSON keys; `deny_unknown_fields`; I/O errors carry path.
- `manifest_schema_version.rs` — wrong `schema_version` returns
  `UnsupportedSchemaVersion(N)`; probe fires before unknown-field check.
- `restore_metadata_roundtrip.rs` — same for `RestoreMetadata`.
- `deferred_capture_and_restore.rs` — `capture()` and `restore()` return
  `SnapshotError::Deferred`.
