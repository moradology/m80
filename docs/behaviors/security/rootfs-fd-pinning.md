# Rootfs fd pinning

Preflight opens the configured rootfs with `O_RDONLY | O_NOFOLLOW`, hashes
that descriptor against the manifest's `output_rootfs_sha256`, rewinds it, and
keeps the descriptor in `Discovery`.

Launch uses the pinned descriptor's `/proc/<m80-pid>/fd/<fd>` path for the
read-only rootfs bind. It does not re-open the original rootfs pathname after
preflight, so replacing the artifact path after verification cannot change the
bytes presented to Firecracker.

Artifact directories used by the configured artifact path must not be group- or
world-writable. Preflight fails closed before launch when the configured
artifact directory or the resolved rootfs parent has `0o020` or `0o002` set.
The rootfs file itself must also not be group- or world-writable; fd pinning
prevents path replacement, not writes to the same inode.
