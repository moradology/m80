# Snapshot Bind Scope

Behavior capture for bead `m80-8emae.29`.

## Contract

`RunningSandbox::capture` and `Sandbox::launch_from_snapshot` expose snapshot
files to jailed Firecracker through a pre-launch `/snapshot` bind. The bind is
part of jailer materialization because Firecracker runs in the jailer's mount
namespace; host bind mounts created after launch are not visible inside that
namespace.

Before accepting caller-provided snapshot paths, `m80-firecracker`
canonicalizes both the backend `run_root` and the snapshot parent and requires
the parent to live below `run_root`.

This rejects caller-provided snapshot paths whose parent is the run root itself
or outside the run root. It also rejects symlink escapes where a path textually
under the run root resolves to a directory elsewhere on the host.

Backend startup recovery owns top-level `run_root` children that look like VM
run-dir names. Durable caller-owned snapshots that must survive a fresh
`Backend` must therefore live under a reserved non-run-dir subtree, such as
`run_root/warm/...`, rather than directly under `run_root`.

Cold launches bind a per-run staging directory read-write at `/snapshot`.
`RunningSandbox::capture` asks Firecracker to write the pair there, moves the
pair to the validated caller path after Firecracker reports success, and writes
the manifest beside the caller-visible pair. Restore binds the validated
snapshot parent read-only at `/snapshot` before launching the restore target.

## Non-Contract

This is a host path containment rule, not a snapshot store policy. It does not
define long-term archive layout, remote storage, or snapshot retention.

## Verification

- `crates/m80-firecracker/src/lifecycle.rs::tests::snapshot_parent_scope_accepts_directory_under_run_root`
- `crates/m80-firecracker/src/lifecycle.rs::tests::snapshot_parent_scope_rejects_directory_outside_run_root`
- `crates/m80-firecracker/src/lifecycle.rs::tests::snapshot_parent_scope_rejects_run_root_itself`
- `crates/m80-firecracker/src/lifecycle.rs::tests::snapshot_parent_scope_rejects_symlink_escape_from_run_root`
- `crates/m80-firecracker/tests/snapshot_concurrency_real_kvm.rs::snapshot_artifact_dirs_stay_under_reserved_warm_tree`
- `cargo test -p m80-firecracker --test snapshot_integration capture_then_restore_round_trip -- --ignored --nocapture`
