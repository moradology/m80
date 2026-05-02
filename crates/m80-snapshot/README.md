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
  silent overwrite.

### Manifest

- `SnapshotManifest` carries five hash-bearing artifacts: VM state file,
  memory image, runtime rootfs clone, workspace scratch image,
  boot-identity record. Optional artifacts: diagnostics bundle, metrics
  snapshot.
- `RestoreMetadata` carries the source identity (`source_workspace_id`,
  `source_run_id`, `source_vm_id`), the snapshot path, and the
  expected-Firecracker-version pin. A restore against a different
  Firecracker version fails closed.
- Restore must materialize into a **fresh VM identity** — the restored
  `vm_id` is required to differ from the source. v0.1 documents this
  invariant; v0.2 enforces it in the execution lane.

### v0.1 surface

In v0.1 the crate exposes only the schemas + path helpers:

- `SnapshotManifest::write(path: &Path) / read(path: &Path)`.
- `RestoreMetadata::write / read`.
- `persistence_path(store_root, workspace_id, run_id, created_at_ms,
  artifact_set_sha256) -> PathBuf`.
- `artifact_set_sha256(&[Artifact]) -> [u8; 32]` — canonical hash over
  the artifact set in declared order.

Capture and restore are `unimplemented!()` in v0.1 with a `Deferred`
error variant exposed for callers to detect.

## Public surface

- `SnapshotManifest`, `RestoreMetadata`, `Artifact`, `ArtifactKind`.
- `persistence_path(...)`.
- `artifact_set_sha256(...)`.
- `capture(...)` and `restore(...)` — present in v0.1 but return
  `SnapshotError::Deferred`.
- `SnapshotError`: `Deferred`, `DestinationCollision`,
  `FirecrackerVersionMismatch`, `Sha256Mismatch`, `Io(io::Error)`,
  `Json(serde_json::Error)`.

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

- Path template: a fixed input set produces the exact documented output.
- Schema round-trip: all manifest variants encode and decode unchanged.
- Sha256 invariance: artifact-set order does not affect the hash beyond
  declared canonical ordering.
- Deferred surfacing: `capture()` and `restore()` return
  `SnapshotError::Deferred` in v0.1.
