# Rootfs fd pinning

Preflight opens the configured rootfs with `O_RDONLY | O_NOFOLLOW`, hashes
that descriptor against the manifest's `output_rootfs_sha256`, rewinds it, and
keeps the descriptor in `Discovery`.

Launch uses the pinned descriptor's `/proc/<m80-pid>/fd/<fd>` path for the
read-only rootfs bind. It does not re-open the original rootfs pathname after
preflight, so replacing the artifact path after verification cannot change the
bytes presented to Firecracker.

`m80-jailer` preserves `/proc/<pid>/fd/<fd>` bind sources as proc-fd paths
instead of canonicalizing them back to the original pathname before
materialization.

Artifact directories used by the configured artifact path must not be group- or
world-writable. Preflight fails closed before launch when the configured
artifact directory or the resolved rootfs parent has `0o020` or `0o002` set.
The rootfs file itself must also not be group- or world-writable; fd pinning
prevents path replacement, not writes to the same inode.

Tests:

- `crates/m80-preflight/tests/security/rootfs_fd_pinning.rs`
- `crates/m80-jailer/src/plan.rs::tests::proc_fd_bind_sources_are_not_canonicalized`
- `crates/m80-jailer/tests/integration_root.rs::materialize_binds_proc_fd_source_without_reopening_original_path`
