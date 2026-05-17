# Overlay Clone Policy Comparison

## Substrate

- host filesystem: `ext4` mounted at `/` from `/dev/nvme1n1p2`
- filesystem options: `rw,relatime,nodioread_nolock,nodelalloc`
- kernel: `6.17.0-23-generic`
- run root: `/var/tmp/m80-overlay-clone-policy-comparison`
- artifact: `/tank/projects/m80/docs/perf/overlay-clone-policy-comparison.md`
- commit: `55e40f78e2374fc0f265a5f22968fb34a4f856ea`
- git worktree dirty excluding this artifact: `true`
- substrate kind: `storage-only`
- overlay size bytes: `67108864`
- template allocated bytes after hole digging: `4440064`
- serial samples: `50`
- concurrent byte-copy: `20` rounds at concurrency `8`
- reflink probe: `unsupported: cp: failed to clone '/var/tmp/m80-overlay-clone-policy-comparison/reflink-probe.ext4' from '/var/tmp/m80-overlay-clone-policy-comparison/template.ext4': Operation not supported`

## Serial Clone Results

| cell | p50_ms | p95_ms | p99_ms | min_ms | max_ms | allocated_p50_bytes |
|---|---:|---:|---:|---:|---:|---:|
| current_rootfs_prepare | 7.164 | 8.508 | 8.592 | 6.887 | 8.592 | 184320 |
| byte_copy_sparse_always | 7.056 | 8.073 | 12.588 | 6.724 | 12.588 | 184320 |
| byte_copy_sparse_auto | 1.737 | 3.042 | 3.086 | 1.602 | 3.086 | 184320 |
| reflink_always | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported: cp: failed to clone '/var/tmp/m80-overlay-clone-policy-comparison/reflink-probe.ext4' from '/var/tmp/m80-overlay-clone-policy-comparison/template.ext4': Operation not supported |
| reflink_auto | 1.703 | 2.968 | 3.269 | 1.564 | 3.269 | 184320 |


## Concurrent Byte-Copy Results

| cell | clones | concurrency | rounds | per_clone_p50_ms | per_clone_p95_ms | per_clone_p99_ms | round_wall_p50_ms | round_wall_p95_ms | round_wall_p99_ms |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| concurrent_byte_copy | 160 | 8 | 20 | 13.813 | 15.976 | 16.834 | 17.611 | 20.043 | 20.217 |

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
