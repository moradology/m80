# Snapshot Manifest Schemas

Behavior capture for the snapshot sidecar schema used by `m80-snapshot`.

## Reserved Names

Every persisted snapshot directory reserves `snapshot-manifest.json` for the
active integrity sidecar. `restore-metadata.json` remains reserved for future
restore orchestration state.

Verification:

- `crates/m80-snapshot/src/tests/manifest_roundtrip.rs::round_trip_equals_original`
- `crates/m80-snapshot/src/tests/restore_metadata_roundtrip.rs::round_trip_equals_original`

## Manifest Validation

`SnapshotManifest` is a typed `#[serde(deny_unknown_fields)]` schema with:

- `artifact_set_sha256`
- `artifacts`
- `created_at_unix_ms`
- `expected_firecracker_version`
- `schema_version`

The active artifact set contains exactly the Firecracker snapshot pair m80
hands to `PUT /snapshot/load`:

- `Memory`
- `VmState`

Each artifact records kind, path, sha256, and byte size. `SnapshotManifest::read`
probes `schema_version` before full parse so future schema versions fail as
`UnsupportedSchemaVersion(N)` before `deny_unknown_fields` can report unrelated
new fields.

Verification:

- `crates/m80-snapshot/src/tests/manifest_roundtrip.rs::required_artifact_kinds_round_trip`
- `crates/m80-snapshot/src/tests/manifest_roundtrip.rs::deny_unknown_fields_rejects_extra_key`
- `crates/m80-snapshot/src/tests/manifest_schema_version.rs::wrong_schema_version_returns_unsupported`

## Capture And Restore Enforcement

Capture writes the manifest after Firecracker reports successful snapshot
creation. Restore reads the manifest, recomputes both artifact sha256s and the
artifact-set digest, compares the manifest Firecracker version against the
restore environment, then checks the live restore-target Firecracker `/version`
converted into m80's `v`-prefixed pin form before unlinking the stale vsock
socket or calling `PUT /snapshot/load`.

Verification:

- `crates/m80-snapshot/tests/capture.rs::capture_writes_manifest_after_snapshot_create`
- `crates/m80-snapshot/tests/restore.rs::restore_rejects_tampered_memory_before_load`
- `crates/m80-snapshot/tests/restore.rs::restore_rejects_firecracker_version_mismatch_before_load`
- `crates/m80-snapshot/tests/version_validation.rs::version_mismatch_is_rejected_before_load`

## Restore Metadata

`RestoreMetadata` remains a typed schema for future persisted restore
orchestration. It records source identity, snapshot path, and Firecracker
version pin, but the active `restore()` integrity path relies on
`SnapshotManifest`.

Verification:

- `crates/m80-snapshot/src/tests/restore_metadata_roundtrip.rs::round_trip_equals_original`
- `crates/m80-snapshot/src/tests/restore_metadata_roundtrip.rs::wrong_schema_version_returns_unsupported`
