# Overlay Clone Policy Comparison

## Substrate

- host filesystem: `zfs` mounted at `/tank` from `tank`
- filesystem options: `rw,noatime,xattr,posixacl,casesensitive`
- kernel: `6.17.0-23-generic`
- run root: `/tank/tmp/m80-overlay-clone-policy-comparison-1g-noise`
- artifact: `/tank/projects/m80/docs/perf/overlay-clone-policy-comparison-1g-noise-zfs.md`
- commit: `55e40f78e2374fc0f265a5f22968fb34a4f856ea`
- git worktree dirty excluding this artifact: `true`
- substrate kind: `storage-only`
- overlay size bytes: `1073741824`
- template physical fill bytes: `1073741824`
- template allocated bytes after hole digging: `857281024`
- serial samples: `30`
- concurrent byte-copy: `5` rounds at concurrency `4`
- reflink probe: `supported`

## Serial Clone Results

| cell | p50_ms | p95_ms | p99_ms | min_ms | max_ms | allocated_p50_bytes |
|---|---:|---:|---:|---:|---:|---:|
| current_rootfs_prepare | 50.472 | 51.608 | 51.695 | 49.694 | 51.695 | 512 |
| byte_copy_sparse_always | 556.757 | 620.367 | 720.231 | 512.194 | 720.231 | 36569600 |
| byte_copy_sparse_auto | 528.923 | 606.477 | 636.061 | 501.619 | 636.061 | 512 |
| reflink_always | 82.707 | 84.372 | 87.118 | 81.255 | 87.118 | 512 |
| reflink_auto | 83.131 | 87.319 | 98.194 | 81.657 | 98.194 | 512 |


## Concurrent Byte-Copy Results

| cell | clones | concurrency | rounds | per_clone_p50_ms | per_clone_p95_ms | per_clone_p99_ms | round_wall_p50_ms | round_wall_p95_ms | round_wall_p99_ms |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| concurrent_byte_copy | 20 | 4 | 5 | 4810.323 | 13372.072 | 13373.618 | 4859.790 | 13405.580 | 13405.580 |

## Residue

- mount entries under run root before teardown: `0`
- run root removed: `true`

## Interpretation

This artifact compares explicit overlay-template clone commands without changing
production behavior. `current_rootfs_prepare` includes m80's current template
validation, explicit byte-copy selection, and `Rootfs::prepare` call overhead.
When `template physical fill bytes` is non-zero, `current_rootfs_prepare`
remains m80's current sparse-template control; the explicit `cp` rows copy the
noise-filled template. `byte_copy_sparse_auto` is m80's current explicit
byte-copy command. `byte_copy_sparse_always` is retained as a historical
comparison against forced sparse scanning. `reflink_auto` is included only as
a GNU cp auto-degrade reference; it is not the desired explicit policy model.

For empty sparse templates, explicit byte-copy below 20 ms P50 / 40 ms P95
makes deleting the implicit fallback machinery performance-plausible on this host.
When `template physical fill bytes` is non-zero, this artifact is a copy-scaling
reference rather than an empty-template policy gate.
