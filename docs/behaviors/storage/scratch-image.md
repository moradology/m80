# Storage Scratch Image

## hydrate

When a caller supplies a host workspace, `m80-storage` creates a per-VM scratch
ext4 image and hydrates it from that host workspace before boot. The guest sees
only the mounted block device; it does not see the original host path.

Current m80 uses `mkfs.ext4 -F`, loop-mounts the scratch image, copies regular
files and directories into the mounted filesystem, and unmounts before launch.
Symlinks, fifos, sockets, device nodes, and unsupported file types are refused
with `StorageError::AdmissibilityRefused` before hydration can complete.

Source: predecessor
`crates/sandbox/agent-sandbox-firecracker/src/storage.rs`
`prepare_workspace_scratch` lines 178-181 used `mkfs.ext4 -d` for the same
observable first-mount hydration behavior. m80 intentionally keeps the behavior
but implements it through a loop-mounted copy path so the admissibility rule is
shared with extraction.

Test: `crates/m80-storage/tests/storage/scratch_image.rs::hydrates_from_host_workspace`.

## sizing

`m80-storage` computes recommended scratch capacity as:

```text
max(64 MiB, used_bytes + 32 MiB), rounded up to a 4 MiB boundary
```

`used_bytes` is the sum of regular-file lengths in the host workspace tree.
Directories do not add payload bytes. Symlinks and special files are refused,
matching hydration.

Source: predecessor
`crates/sandbox/agent-sandbox-firecracker/src/storage.rs` constants
`MIN_SCRATCH_BYTES`, `SCRATCH_PADDING_BYTES`, `SCRATCH_ALIGNMENT_BYTES` and
`aligned_scratch_size` lines 688-694.

Test: `crates/m80-storage/tests/storage/scratch_image.rs::sizing_obeys_padding_and_alignment`.
