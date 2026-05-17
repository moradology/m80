# Overlay Clone Policy Comparison

## Substrate

- host filesystem: `tmpfs` mounted at `/dev/shm` from `tmpfs`
- filesystem options: `rw,nosuid,nodev,inode64`
- kernel: `6.17.0-23-generic`
- run root: `/dev/shm/m80-overlay-clone-policy-comparison`
- artifact: `/tank/projects/m80/docs/perf/overlay-clone-policy-comparison-tmpfs.md`
- commit: `55e40f78e2374fc0f265a5f22968fb34a4f856ea`
- git worktree dirty excluding this artifact: `true`
- substrate kind: `storage-only`
- overlay size bytes: `67108864`
- template allocated bytes after hole digging: `180224`
- serial samples: `50`
- concurrent byte-copy: `20` rounds at concurrency `8`
- reflink probe: `unsupported: cp: failed to clone '/dev/shm/m80-overlay-clone-policy-comparison/reflink-probe.ext4' from '/dev/shm/m80-overlay-clone-policy-comparison/template.ext4': Operation not supported`

## Serial Clone Results

| cell | p50_ms | p95_ms | p99_ms | min_ms | max_ms | allocated_p50_bytes |
|---|---:|---:|---:|---:|---:|---:|
| current_rootfs_prepare | 1.409 | 2.368 | 2.760 | 1.291 | 2.760 | 180224 |
| byte_copy_sparse_always | 1.371 | 2.391 | 2.437 | 1.289 | 2.437 | 180224 |
| byte_copy_sparse_auto | 1.350 | 2.389 | 2.621 | 1.276 | 2.621 | 180224 |
| reflink_always | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported: cp: failed to clone '/dev/shm/m80-overlay-clone-policy-comparison/reflink-probe.ext4' from '/dev/shm/m80-overlay-clone-policy-comparison/template.ext4': Operation not supported |
| reflink_auto | 1.292 | 2.300 | 2.680 | 1.194 | 2.680 | 180224 |


## Concurrent Byte-Copy Results

| cell | clones | concurrency | rounds | per_clone_p50_ms | per_clone_p95_ms | per_clone_p99_ms | round_wall_p50_ms | round_wall_p95_ms | round_wall_p99_ms |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| concurrent_byte_copy | 160 | 8 | 20 | 1.659 | 1.951 | 2.632 | 5.231 | 5.794 | 6.688 |

## Residue

- mount entries under run root before teardown: `0`
- run root removed: `true`

## Interpretation

This artifact compares explicit overlay-template clone commands without changing
production behavior. `current_rootfs_prepare` includes m80's current template
validation, explicit byte-copy selection, and `Rootfs::prepare` call overhead.
`byte_copy_sparse_auto` is m80's current explicit byte-copy command.
`byte_copy_sparse_always` is retained as a historical comparison against forced
sparse scanning. `reflink_auto` is included only as
a GNU cp auto-degrade reference; it is not the desired explicit policy model.

For empty sparse templates, explicit byte-copy below 20 ms P50 / 40 ms P95
makes deleting the implicit fallback machinery performance-plausible on this host.
When `template physical fill bytes` is non-zero, this artifact is a copy-scaling
reference rather than an empty-template policy gate.
