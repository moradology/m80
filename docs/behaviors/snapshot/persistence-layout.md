# Snapshot persistence layout

Source: predecessor `docs/gates/stage-g-firecracker-snapshot-persistence-contract.md`; m80 crate `m80-snapshot`.

---

## path-template {#path-template}

The system lays out persisted snapshot sets at
`<store-root>/<workspace_id>/<run_id>/<created_at_unix_ms>-<artifact_set_sha256>/`.

**Present-tense statement.** `persistence_path(store_root, workspace_id, run_id, created_at_unix_ms, artifact_set_sha256)`
returns a `PathBuf` equal to
`store_root.join(workspace_id).join(run_id).join("{created_at_unix_ms}-{artifact_set_sha256}")`.
This is pure path construction — no I/O is performed.  The caller is responsible
for providing values that produce a valid path component (e.g., no embedded `/`
in IDs unless intentional nesting is desired).

Tools that walk the snapshot store rely on this template; it must not change
without a major version bump.

**predecessor source.**
- `docs/gates/stage-g-firecracker-snapshot-persistence-contract.md` §Contract rule 4
  (deterministic backend-local layout).

**m80 test.** `crates/m80-snapshot/tests/persistence_path.rs::exact_output_matches_documented_template`

---

## collision-fail-closed {#collision-fail-closed}

The system refuses to overwrite an existing persisted snapshot directory and
returns an explicit collision error rather than replacing artifacts in place.

**Present-tense statement.** `SnapshotError::DestinationCollision` is the
designated error variant for this case.  In v0.1 `capture()` is not
implemented (returns `SnapshotError::Deferred`), so the collision check is not
exercised at runtime.  The variant is present in `SnapshotError` so callers can
write match arms for it today; v0.2 execution will produce it.

**predecessor source.**
- `docs/gates/stage-g-firecracker-snapshot-persistence-contract.md` §Contract rule 7.

**m80 test.** `SnapshotError::DestinationCollision` variant present — verified by
`crates/m80-snapshot/tests/deferred_capture_and_restore.rs` (capture returns
`Deferred`, not `DestinationCollision`, in v0.1).

---

## host-local-only {#host-local-only}

The system supports only host-local filesystem persistence in v0.1 and
explicitly does not introduce S3, GCS, Azure, `object_store`, or any generic
storage trait.

**Present-tense statement.** `persistence_path` takes a `&Path` for the store
root and returns a `PathBuf`.  There is no storage-backend trait, no `object_store`
dependency, no URL scheme handling.  Adding remote stores is a v0.2+ epic;
v0.1 ships no seam for it.  The `m80-snapshot` dependency list contains only
`serde`, `serde_json`, `sha2`, `hex`, and `thiserror`.

**predecessor source.**
- `docs/gates/stage-g-firecracker-snapshot-persistence-contract.md` §Contract rule 1.
- Dossier `00-verdict.md` §Why this works #4.

**m80 test.** `crates/m80-snapshot/tests/persistence_path.rs` — all tests pass
a plain `&Path`; no URL or remote-scheme type appears anywhere in the crate.
