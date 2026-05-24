# Track Dirty Pages

## Behavior

`m80-firecracker-client::MachineConfig` carries
`track_dirty_pages: Option<bool>`. The field is omitted from `PUT
/machine-config` when `None`, which is the current `m80-firecracker` preboot
default. Future diff-snapshot launch paths can set it to `Some(true)` without
changing the Firecracker REST speaker shape again.

`m80-snapshot::RestoreRequest` carries `enable_diff_snapshots: bool`. Restore
omits Firecracker's `enable_diff_snapshots` field when this is `false`, and
serializes `"enable_diff_snapshots":true` in `PUT /snapshot/load` when it is
`true`.

`m80-snapshot::CaptureRequest` carries the same invariant marker. A
`SnapshotKind::Diff` capture with `enable_diff_snapshots = false` fails before
REST calls, because a diff snapshot is only meaningful when dirty-page tracking
was intentionally enabled for that snapshot lineage.

## Current Orchestrator Defaults

`m80-firecracker` sets `MachineConfig::track_dirty_pages = None` during preboot
and uses `enable_diff_snapshots = false` on current restore paths. This leaf
adds typed plumbing only; it does not enable production diff snapshots.

## Tests

- `crates/m80-firecracker-client/tests/put_each_resource.rs` pins
  `track_dirty_pages` serialization on `MachineConfig`.
- `crates/m80-firecracker-client/tests/snapshot.rs` pins
  `LoadSnapshotConfig::enable_diff_snapshots = Some(true)` serialization.
- `crates/m80-snapshot/tests/dirty_pages_plumbing.rs` pins restore request-body
  omission for `false` and serialization for `true`.
- `crates/m80-snapshot/tests/capture.rs` pins the fail-closed diff-capture
  invariant.
