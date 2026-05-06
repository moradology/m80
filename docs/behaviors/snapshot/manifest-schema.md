# Snapshot manifest schemas

Source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/snapshot.rs`; m80 crate `m80-snapshot`.

---

## reserved-names {#reserved-names}

The system reserves the names `snapshot-manifest.json` and
`restore-metadata.json` in every persisted snapshot directory.

**Present-tense statement.** `m80-snapshot` exposes the constants
`SNAPSHOT_MANIFEST_FILE = "snapshot-manifest.json"` and
`RESTORE_METADATA_FILE = "restore-metadata.json"`.  Both names are
locked so that any tool writing a snapshot today uses the same
paths that active restore code will read.  The constants pin the on-disk
contract; callers must not derive these strings independently.

**predecessor source.**
- `snapshot.rs` line 13: `pub const SNAPSHOT_MANIFEST_FILE: &str = "snapshot-manifest.json";`
- `snapshot.rs` line 14: `pub const RESTORE_METADATA_FILE: &str = "restore-metadata.json";`
- `docs/gates/stage-g-firecracker-snapshot-restore-contract.md` §Current Boundary.

**m80 test.** `crates/m80-snapshot/tests/manifest_roundtrip.rs` — writes and reads back
`snapshot-manifest.json` by name; `crates/m80-snapshot/tests/restore_metadata_roundtrip.rs`
— same for `restore-metadata.json`.

---

## manifest-validation {#manifest-validation}

The system encodes `SnapshotManifest` as a typed schema and rejects any
file that does not satisfy its required fields.

**Present-tense statement.** `SnapshotManifest` carries:
- `artifact_set_sha256` — hex sha256 over the ordered artifact set.
- `artifacts` — the five required artifacts (see below) in declared order.
- `created_at_unix_ms` — Unix epoch milliseconds at capture time.
- `expected_firecracker_version` — Firecracker version pin (opaque string).
- `schema_version` — always `SCHEMA_VERSION` (value `1`).
- `source_run_id`, `source_vm_id`, `source_workspace_id` — caller-supplied opaque strings.
- Optional: `diagnostics_bundle`, `metrics_snapshot`.

`SnapshotManifest::read` uses `#[serde(deny_unknown_fields)]` so any extra key
in the file surfaces as `SnapshotError::Json`.  A schema-version mismatch is
detected first via `SchemaVersionProbe` and surfaces as
`SnapshotError::UnsupportedSchemaVersion(v)` before `deny_unknown_fields`
can fire.

The five required artifact kinds are `BootIdentity`, `Memory`, `RuntimeRootfs`,
`VmState`, and `WorkspaceScratch`.  The correct artifact count is the store
writer's responsibility.

**predecessor source.**
- `snapshot.rs` lines 55–63: `FirecrackerSnapshotManifest` struct definition.
- `snapshot.rs` lines 118–175: `validate` enforcing 5-element artifact set.
- `docs/gates/stage-g-firecracker-snapshot-restore-contract.md` §Contract rule 5.

**m80 tests.**
- `crates/m80-snapshot/tests/manifest_roundtrip.rs::five_required_artifact_kinds_round_trip`
- `crates/m80-snapshot/tests/manifest_roundtrip.rs::deny_unknown_fields_rejects_extra_key`
- `crates/m80-snapshot/tests/manifest_schema_version.rs::wrong_schema_version_returns_unsupported`

---

## restore-metadata {#restore-metadata}

The system carries restore metadata as a typed schema with source identity,
snapshot path, and Firecracker version pin.

**Present-tense statement.** `RestoreMetadata` carries:
- `expected_firecracker_version` — Firecracker version the snapshot is pinned to.
- `schema_version` — always `SCHEMA_VERSION` (value `1`).
- `snapshot_path` — path to the snapshot directory.
- `source_run_id`, `source_vm_id`, `source_workspace_id` — source identity.

A restore against a different Firecracker version fails closed at the
orchestrator layer. `m80-snapshot` records and returns the expected version.

The predecessor schema additionally carried `restored_vm_id`, `restored_run_dir`,
`artifact_set_sha256`, `phase` (`Planned|Materialized|Failed`),
`restored_at_unix_ms`, and `last_error`.  Those fields belong to the
orchestrator execution lane; they are not part of `m80-snapshot`'s schema
contract.

**predecessor source.**
- `snapshot.rs` lines 65–70: `FirecrackerRestorePhase` enum.
- `snapshot.rs` lines 73–86: `FirecrackerRestoreMetadata` struct.

**m80 tests.**
- `crates/m80-snapshot/tests/restore_metadata_roundtrip.rs::round_trip_equals_original`
- `crates/m80-snapshot/tests/restore_metadata_roundtrip.rs::wrong_schema_version_returns_unsupported`
