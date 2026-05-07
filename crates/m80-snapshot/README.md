# `m80-snapshot`

Snapshot **manifest schemas**, **persistence path layout**, and
**capture/restore primitives** for Firecracker microVM snapshots.

Schemas, capture, and restore primitives are active. `m80-snapshot`
owns Firecracker REST calls and schema/path helpers; `m80-firecracker`
owns lifecycle state and jail path translation.

## Reason for being

The crate serves two purposes:

1. **Lock the on-disk schema** — `SnapshotManifest`, `RestoreMetadata`, and
   the persistence path template `<store>/<workspace_id>/<run_id>/<unix_ms>-<sha>/`
   are the canonical contract. Tools that walk the store rely on this
   template; changing it after any snapshot is persisted would render those
   snapshots unreadable.

2. **Provide capture/restore primitives** — `capture` and `restore` issue
   the Firecracker REST calls required to pause/snapshot and load/resume a
   microVM. These are thin wrappers over `m80-firecracker-client`; they own
   no lifecycle state. The orchestrator (`m80-firecracker`) composes them.

## Black-box contract

### Persistence path

- Snapshots persist at:
  `<store-root>/<workspace_id>/<run_id>/<created_at_unix_ms>-<artifact_set_sha256>/`.
  This template is the canonical layout. Tools that walk the store rely
  on it.
- `<store-root>` is host-local filesystem only. No S3/GCS/remote-store
  support. Adding remote stores is a v0.2+ epic.
- A persistence collision (the destination directory already exists)
  fails closed with `SnapshotError::DestinationCollision`. There is no
  silent overwrite.

### Manifest

- `SnapshotManifest` carries five required artifacts: boot identity record,
  memory image, runtime rootfs clone, VM state file, workspace scratch
  image (declared alphabetically by `ArtifactKind` variant). Optional
  artifacts: diagnostics bundle, metrics snapshot.
- `RestoreMetadata` carries the source identity (`source_workspace_id`,
  `source_run_id`, `source_vm_id`), the snapshot path, and the
  expected-Firecracker-version pin. A restore against a different
  Firecracker version fails closed (enforcement is `m80-firecracker`'s
  job at restore time; the crate records the value only).
- Both files use `#[serde(deny_unknown_fields)]`. An unknown key in a
  v0.2+ file surfaces as `SnapshotError::Json`; a `schema_version`
  mismatch surfaces first as `SnapshotError::UnsupportedSchemaVersion`.

### Capture

`capture(CaptureRequest)`:

1. PATCH `/vm` → `Paused` via `m80-firecracker-client`.
2. PUT `/snapshot/create` with the configured paths and kind.

The VM is left in the `Paused` state. The caller (orchestrator) decides
whether to resume or kill the Firecracker process.

`SnapshotPaths` passed to Firecracker must be visible from the Firecracker
process namespace. For jailed Firecracker, `m80-firecracker` owns that
translation by bind-mounting the host snapshot directory into the jail and
passing in-jail `/snapshot/...` paths to this crate.

### Restore

`restore(RestoreRequest)`:

1. `unlink(vsock_uds)` if present — required because Firecracker rebinds
   the UDS at load time and fails with `EADDRINUSE` if the file exists.
   `ENOENT` is silently ignored; any other error surfaces as
   `SnapshotError::VsockUdsUnlink`.
2. PUT `/snapshot/load` with `mem_backend = File`.
3. If `resume: true`, PATCH `/vm` → `Resumed`.

## Public surface

### Types

- `SnapshotPaths { vm_state: PathBuf, mem: PathBuf }` — the two-file
  artifact pair written at capture time.
- `SnapshotKind` — `Full | Diff`.
- `CaptureRequest { fc_socket: PathBuf, paths: SnapshotPaths, kind: SnapshotKind }`.
- `RestoreRequest { fc_socket: PathBuf, paths: SnapshotPaths, vsock_uds: PathBuf, resume: bool }`.
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

### Functions

- `capture(req: CaptureRequest) -> Result<(), SnapshotError>`.
- `restore(req: RestoreRequest) -> Result<(), SnapshotError>`.
- `persistence_path(store_root, workspace_id, run_id, created_at_ms, artifact_set_sha256) -> PathBuf` — pure path construction, no I/O.
- `artifact_set_sha256(&[Artifact]) -> [u8; 32]` — SHA-256 over
  per-artifact JSON bytes concatenated in slice order.

### Constants

- `SNAPSHOT_MANIFEST_FILE`, `RESTORE_METADATA_FILE`, `SCHEMA_VERSION`.

### Errors

`SnapshotError`:
- `DestinationCollision` — persistence directory already exists.
- `UnsupportedSchemaVersion(u32)` — schema version mismatch on read.
- `Io { path: PathBuf, source: io::Error }` — filesystem I/O failure.
- `Json(serde_json::Error)` — JSON encode/decode failure.
- `Client(m80_firecracker_client::ClientError)` — Firecracker REST failure.
- `VsockUdsUnlink { path: PathBuf, source: io::Error }` — vsock UDS
  removal failed for a reason other than `NotFound`.

## Non-goals

- **No spawning Firecracker processes.** This crate only issues REST calls
  to an already-running Firecracker process via its API socket.
- **No run-dir management.** Creating/destroying run directories is
  `m80-firecracker`'s job.
- **No lifecycle state machine.** The orchestrator (`m80-firecracker`)
  composes `capture` and `restore` within its state machine.
- **No vm-id → CID mapping.** That belongs to `m80-vsock`.
- **No remote store.** Filesystem only.
- **No incremental snapshots** beyond issuing `Diff` to Firecracker.
- **No live migration.**

## Dependencies

- `m80-firecracker-client` — REST client for Firecracker's API.
- `serde`, `serde_json`, `sha2`, `thiserror`.

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
- `fixture_server.rs` — shared fixture HTTP server (UnixListener) used by
  capture/restore tests; no real Firecracker binary required.
- `capture.rs` — wire-call ordering and body shape for `capture`; pause
  failure and create failure error paths. Each scenario its own `#[test]`.
- `restore.rs` — wire-call ordering, File-backed mem_backend body shape,
  vsock UDS removal (absent / present / unremovable), load and resume
  error paths. Each scenario its own `#[test]`.
