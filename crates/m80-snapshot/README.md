# `m80-snapshot`

Snapshot **capture/restore primitives** plus private manifest-schema helpers
for Firecracker microVM snapshots.

Capture and restore primitives are active. Manifest schemas and persistence
path helpers are pinned internally, but they are not public API until
`m80-firecracker` actually writes/reads persisted snapshot manifests.
`m80-snapshot` owns Firecracker REST calls; `m80-firecracker` owns lifecycle
state and jail path translation.

## Reason for being

The crate serves two purposes:

1. **Pin the future on-disk schema privately** — `SnapshotManifest`,
   `RestoreMetadata`, and the persistence path template
   `<store>/<workspace_id>/<run_id>/<unix_ms>-<sha>/` are tested inside the
   crate so the eventual persistence integration has a concrete shape. They
   are not exported while no production caller writes or reads those files.

2. **Provide capture/restore primitives** — `capture` and `restore` issue
   the Firecracker REST calls required to pause/snapshot and load/resume a
   microVM. These are thin wrappers over `m80-firecracker-client`; they own
   no lifecycle state. The orchestrator (`m80-firecracker`) composes them.

## Black-box contract

### Private persistence path

- Snapshots persist at:
  `<store-root>/<workspace_id>/<run_id>/<created_at_unix_ms>-<artifact_set_sha256>/`.
  This template is pinned by private tests, not exposed as public API yet.
- `<store-root>` is host-local filesystem only. No S3/GCS/remote-store
  support. Adding remote stores is a v0.2+ epic.

### Private manifest

- `SnapshotManifest` carries five required artifacts: boot identity record,
  memory image, runtime rootfs clone, VM state file, workspace scratch
  image (declared alphabetically by `ArtifactKind` variant). Optional
  artifacts: diagnostics bundle, metrics snapshot.
- `RestoreMetadata` carries the source identity (`source_workspace_id`,
  `source_run_id`, `source_vm_id`), the snapshot path, and the
  expected-Firecracker-version pin. A restore against a different
  Firecracker version fails closed (enforcement is `m80-firecracker`'s
  job at restore time; the crate records the value only).
- Both files use `#[serde(deny_unknown_fields)]`. Private schema tests assert
  that an unknown key surfaces as a schema JSON error and a `schema_version`
  mismatch surfaces first as an unsupported-schema-version error.

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
- `CaptureRequest { api_socket: PathBuf, paths: SnapshotPaths, kind: SnapshotKind }`.
- `RestoreRequest { api_socket: PathBuf, paths: SnapshotPaths, vsock_uds: PathBuf, resume: bool }`.

### Functions

- `capture(req: CaptureRequest) -> Result<(), SnapshotError>`.
- `restore(req: RestoreRequest) -> Result<(), SnapshotError>`.

### Errors

`SnapshotError`:
- `Client(m80_firecracker_client::ClientError)` — Firecracker REST failure.
- `VsockUdsUnlink { path: PathBuf, source: io::Error }` — vsock UDS
  removal failed for a reason other than `NotFound`.

## Non-goals

- **No spawning Firecracker processes.** This crate only issues REST calls
  to an already-running Firecracker process via its API socket.
- **No run-dir management.** Creating/destroying run directories is
  `m80-firecracker`'s job.
- **No public manifest persistence API yet.** Schema structs and path helpers
  remain crate-private until the orchestrator has production manifest
  write/read integration.
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

Tests:

- `src/tests/persistence_path.rs` — exact-output test for the private
  persistence path helper.
- `src/tests/artifact_set_sha256.rs` — determinism, sensitivity to per-field and
  order changes.
- `src/tests/manifest_roundtrip.rs` — private `SnapshotManifest::write` then `read`
  produces equal struct; byte-stable round-trip; mode 0644 on Unix;
  alphabetical JSON keys; `deny_unknown_fields`; I/O errors carry path.
- `src/tests/manifest_schema_version.rs` — wrong `schema_version` returns
  `UnsupportedSchemaVersion(N)`; probe fires before unknown-field check.
- `src/tests/restore_metadata_roundtrip.rs` — same for private `RestoreMetadata`.
- `fixture_server.rs` — shared fixture HTTP server (UnixListener) used by
  capture/restore tests; no real Firecracker binary required.
- `capture.rs` — wire-call ordering and body shape for `capture`; pause
  failure and create failure error paths. Each scenario its own `#[test]`.
- `restore.rs` — wire-call ordering, File-backed mem_backend body shape,
  vsock UDS removal (absent / present / unremovable), load and resume
  error paths. Each scenario its own `#[test]`.
