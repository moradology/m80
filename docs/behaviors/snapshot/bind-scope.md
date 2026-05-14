# Snapshot Bind Scope

Behavior capture for bead `m80-8emae.29`.

## Contract

`RunningSandbox::capture` and `Sandbox::launch_from_snapshot` bind the snapshot
pair parent into the Firecracker jail at `/snapshot`. Before changing ownership
or bind-mounting that parent, `m80-firecracker` canonicalizes both the backend
`run_root` and the snapshot parent and requires the parent to live below
`run_root`.

This rejects caller-provided snapshot paths whose parent is the run root itself
or outside the run root. It also rejects symlink escapes where a path textually
under the run root resolves to a directory elsewhere on the host. The helper
uses the canonical accepted parent as the `chown` and bind-mount source.

## Non-Contract

This is a host path containment rule, not a snapshot store policy. It does not
define long-term archive layout, remote storage, or snapshot retention.

## Verification

- `crates/m80-firecracker/src/lifecycle.rs::tests::snapshot_parent_scope_accepts_directory_under_run_root`
- `crates/m80-firecracker/src/lifecycle.rs::tests::snapshot_parent_scope_rejects_directory_outside_run_root`
- `crates/m80-firecracker/src/lifecycle.rs::tests::snapshot_parent_scope_rejects_run_root_itself`
- `crates/m80-firecracker/src/lifecycle.rs::tests::snapshot_parent_scope_rejects_symlink_escape_from_run_root`
