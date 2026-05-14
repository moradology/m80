# Snapshot Persistence Path Validation

Behavior capture for bead `m80-8emae.17`.

## Contract

`m80_snapshot::persistence_path` builds snapshot persistence directories as:

```text
<store-root>/<workspace_id>/<run_id>/<created_at_unix_ms>-<artifact_set_sha256>/
```

`workspace_id` and `run_id` are caller-supplied identifiers, so each must be a
single visible path component. The helper rejects:

- empty strings
- leading `.`
- `/`
- `\`
- NUL bytes
- `..`

Rejection is typed as `SnapshotError::InvalidId { field, value }`. The helper
performs no filesystem I/O; it only validates IDs and constructs the path.

## Non-Contract

This helper does not validate `store_root`; the caller chooses the snapshot
store. It also does not validate `artifact_set_sha256`, which is a computed
digest in the snapshot pipeline rather than an opaque caller ID.

## Verification

- `crates/m80-snapshot/src/tests/persistence_path.rs`
- `crates/m80-snapshot/tests/persistence_path_validation.rs`
