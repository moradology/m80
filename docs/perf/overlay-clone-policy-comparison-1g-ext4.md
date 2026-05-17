# Overlay Clone Policy Comparison

## Substrate

- host filesystem: `ext4` mounted at `/` from `/dev/nvme1n1p2`
- filesystem options: `rw,relatime,nodioread_nolock,nodelalloc`
- kernel: `6.17.0-23-generic`
- run root: `/var/tmp/m80-overlay-clone-policy-comparison-1g`
- artifact: `/tank/projects/m80/docs/perf/overlay-clone-policy-comparison-1g-ext4.md`
- commit: `55e40f78e2374fc0f265a5f22968fb34a4f856ea`
- git worktree dirty excluding this artifact: `true`
- substrate kind: `storage-only`
- overlay size bytes: `1073741824`
- template allocated bytes after hole digging: `34504704`
- serial samples: `50`
- concurrent byte-copy: `20` rounds at concurrency `8`
- reflink probe: `unsupported: cp: failed to clone '/var/tmp/m80-overlay-clone-policy-comparison-1g/reflink-probe.ext4' from '/var/tmp/m80-overlay-clone-policy-comparison-1g/template.ext4': Operation not supported`

## Serial Clone Results

| cell | p50_ms | p95_ms | p99_ms | min_ms | max_ms | allocated_p50_bytes |
|---|---:|---:|---:|---:|---:|---:|
| current_rootfs_prepare | 9.817 | 11.637 | 12.464 | 9.041 | 12.464 | 888832 |
| byte_copy_sparse_always | 9.588 | 11.158 | 12.264 | 9.350 | 12.264 | 888832 |
| byte_copy_sparse_auto | 3.306 | 4.634 | 5.476 | 3.168 | 5.476 | 888832 |
| reflink_always | unsupported | unsupported | unsupported | unsupported | unsupported | unsupported: cp: failed to clone '/var/tmp/m80-overlay-clone-policy-comparison-1g/reflink-probe.ext4' from '/var/tmp/m80-overlay-clone-policy-comparison-1g/template.ext4': Operation not supported |
| reflink_auto | 3.071 | 3.873 | 4.419 | 2.943 | 4.419 | 888832 |


## Concurrent Byte-Copy Results

| cell | clones | concurrency | rounds | per_clone_p50_ms | per_clone_p95_ms | per_clone_p99_ms | round_wall_p50_ms | round_wall_p95_ms | round_wall_p99_ms |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| concurrent_byte_copy | 160 | 8 | 20 | 21.810 | 22.716 | 23.784 | 26.542 | 28.260 | 28.376 |

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
