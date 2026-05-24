# `m80-snapshot`

Snapshot capture/restore primitives plus manifest-schema helpers for
Firecracker microVM snapshots.

Capture writes a `snapshot-manifest.json` sidecar for the Firecracker snapshot
pair. Normal restore reads that manifest before `PUT /snapshot/load`,
recomputes the pair sha256s, and fails closed on artifact or
Firecracker-version mismatch.
`m80-snapshot` owns Firecracker REST calls; `m80-firecracker` owns lifecycle
state and jail path translation.

## Reason for being

The crate serves two purposes:

1. **Pin the active on-disk schema** — `SnapshotManifest` records the memory
   image and VM state file sha256s plus the Firecracker version pin beside the
   snapshot pair. `RestoreMetadata` and the persistence path template
   `<store>/<workspace_id>/<run_id>/<unix_ms>-<sha>/` remain internal
   scaffolding for future persisted restore orchestration.
2. **Provide capture/restore primitives** — `capture` and `restore` issue the
   Firecracker REST calls required to pause/snapshot and load/resume a microVM.
   These are thin wrappers over `m80-firecracker-client`; they own no lifecycle
   state. The orchestrator composes them.

## Black-box contract

### Persistence path

- Snapshots persist at:
  `<store-root>/<workspace_id>/<run_id>/<created_at_unix_ms>-<artifact_set_sha256>/`.
  `workspace_id` and `run_id` must each be a single non-hidden path component:
  empty strings, `/`, `\`, NUL bytes, `..`, and leading `.` are rejected as
  `SnapshotError::InvalidId`.
- `<store-root>` is host-local filesystem only. No S3/GCS/remote-store
  support. Adding remote stores is a v0.2+ epic.

### Manifest

- `SnapshotManifest` carries two required artifacts in declared order:
  `Memory` and `VmState`. Each artifact records kind, path, size, and sha256.
- The manifest also records `artifact_set_sha256`, `created_at_unix_ms`,
  `expected_firecracker_version`, and `schema_version`.
- Restore recomputes both artifact sha256s and the artifact-set digest before
  touching the stale vsock UDS or Firecracker API socket.
- A restore against a different Firecracker version fails closed before any
  side effect.
- Manifest and restore-metadata files use `#[serde(deny_unknown_fields)]`.
  A schema-version mismatch is detected before full parse so future-version
  files surface as `UnsupportedSchemaVersion(N)`.

### Capture

`capture(CaptureRequest)`:

1. PATCH `/vm` -> `Paused` via `m80-firecracker-client`.
2. PUT `/snapshot/create` with the configured Firecracker-visible paths and kind.
3. Hash the host-readable snapshot pair and write `snapshot-manifest.json`.

The VM is left in the `Paused` state. The caller decides whether to resume or
kill the Firecracker process.
`SnapshotKind::Diff` requires `enable_diff_snapshots = true`; otherwise capture
fails before issuing REST calls. The field records that dirty-page tracking was
intentionally enabled before the diff snapshot request.

`paths` must be visible from the Firecracker process namespace. `host_paths`
must be the same artifact pair as seen by the m80 host process for hashing and
manifest persistence. For jailed Firecracker, `m80-firecracker` bind-mounts the
host snapshot directory into the jail and passes in-jail `/snapshot/...` paths
as `paths` while keeping the caller's host paths in `host_paths`.

### Restore

`restore(RestoreRequest)`:

1. Read `snapshot-manifest.json`, recompute the snapshot pair sha256s, and
   verify `expected_firecracker_version`.
2. GET `/version` from the restore-target Firecracker process, convert the raw
   API value (`1.15.1`) to m80's `v`-prefixed pin form (`v1.15.1`), and require
   it to match `expected_firecracker_version`.
3. `unlink(vsock_uds)` if present. `ENOENT` is ignored; any other error
   surfaces as `SnapshotError::VsockUdsUnlink`.
4. PUT `/snapshot/load` with `mem_backend = File`; when
   `enable_diff_snapshots = true`, include Firecracker's
   `enable_diff_snapshots` field so the restored VM tracks dirty pages for the
   next diff snapshot.
5. If `resume: true`, PATCH `/vm` -> `Resumed`.

`restore_preverified(RestoreRequest)` skips step 1 for callers that already
verified an immutable snapshot body. It still performs the live Firecracker
version check before unlinking stale vsock state or loading the snapshot.
The v0.1 consumer is
`m80-snapshot-template`: template commit hashes the snapshot pair, records the
manifest in the content-addressed body, and pinning validates template identity
before restore. Direct caller-supplied snapshots must keep using `restore`.

## Public surface

### Types

- `SnapshotPaths { vm_state: PathBuf, mem: PathBuf }`.
- `SnapshotKind` — `Full | Diff`.
- `SnapshotManifest`, `Artifact`, `ArtifactKind`, and `SchemaError`.
- `CaptureRequest { api_socket, paths, host_paths, expected_firecracker_version, kind, enable_diff_snapshots }`.
- `RestoreRequest { api_socket, paths, host_paths, expected_firecracker_version, vsock_uds, enable_diff_snapshots, resume }`.

### Functions

- `capture(req: CaptureRequest) -> Result<(), SnapshotError>`.
- `restore(req: RestoreRequest) -> Result<(), SnapshotError>`.
- `restore_preverified(req: RestoreRequest) -> Result<(), SnapshotError>`.
- `write_snapshot_manifest(paths, expected_firecracker_version) -> Result<(), SnapshotError>`.
- `verify_snapshot_manifest(paths, expected_firecracker_version) -> Result<SnapshotManifest, SnapshotError>`.
- `persistence_path(store_root, workspace_id, run_id, created_at_unix_ms, artifact_set_sha256) -> Result<PathBuf, SnapshotError>`.

### Errors

`SnapshotError`:
- `Client(m80_firecracker_client::ClientError)` — Firecracker REST failure.
- `Schema(SchemaError)` — manifest schema/read failure.
- `ArtifactIo { path, source }` — snapshot artifact could not be read.
- `ArtifactMismatch { .. }`, `ArtifactSetMismatch { .. }`,
  `ManifestMissingArtifact { .. }`, and `ManifestArtifactSetInvalid { .. }` —
  integrity failures.
- `FirecrackerVersionMismatch { expected, recorded }` — restore environment
  does not match the capture-time pin.
- `VersionMismatch { expected, actual }` — live Firecracker `/version`,
  converted to m80's `v`-prefixed pin form, does not match the snapshot's
  expected version.
- `DiffSnapshotsDisabled` — a diff capture was requested without dirty-page
  tracking enabled.
- `VsockUdsUnlink { path, source }` — vsock UDS removal failed for a reason
  other than `NotFound`.
- `InvalidId { field, value }` — persistence helper rejected a caller-supplied
  path component.

## Non-goals

- **No spawning Firecracker processes.** This crate only issues REST calls to
  an already-running Firecracker process via its API socket.
- **No run-dir management.** Creating/destroying run directories is
  `m80-firecracker`'s job.
- **No lifecycle state machine.** The orchestrator composes `capture` and
  `restore` within its state machine.
- **No vm-id -> CID mapping.** That belongs to `m80-vsock`.
- **No remote store.** Filesystem only.
- **No incremental snapshots** beyond issuing `Diff` to Firecracker.
- **No live migration.**

## Dependencies

- `m80-firecracker-client` — REST client for Firecracker's API.
- `serde`, `serde_json`, `sha2`, `hex`, `thiserror`.

## Tests

- `src/tests/persistence_path.rs` and `tests/persistence_path_validation.rs` —
  exact-output and invalid-id tests for the persistence path helper.
- `src/tests/artifact_set_sha256.rs` — determinism, sensitivity to per-field
  and order changes.
- `src/tests/manifest_roundtrip.rs` — `SnapshotManifest::write` then `read`
  produces equal struct; byte-stable round-trip; mode 0644 on Unix;
  alphabetical JSON keys; `deny_unknown_fields`; I/O errors carry path.
- `src/tests/manifest_schema_version.rs` — wrong `schema_version` returns
  `UnsupportedSchemaVersion(N)`; probe fires before unknown-field check.
- `src/tests/restore_metadata_roundtrip.rs` — same for private
  `RestoreMetadata`.
- `fixture_server.rs` — shared fixture HTTP server used by capture/restore
  tests; no real Firecracker binary required.
- `capture.rs` — wire-call ordering and body shape for `capture`; manifest
  write; pause failure and create failure error paths.
- `restore.rs` — manifest integrity/version rejection before load, wire-call
  ordering, File-backed mem_backend body shape, vsock UDS removal, load and
  resume error paths.
