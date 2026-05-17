# Ext4 Overlay-Template Clone Measurement

Bead: `m80-q420k.8.16`.

## Substrate

- host filesystem: `ext4` mounted at `/` from `/dev/nvme1n1p2`
- filesystem options: `rw,relatime,nodioread_nolock,nodelalloc`
- kernel: `6.17.0-23-generic`
- run root: `/var/tmp/m80-ext4-overlay-template-clone-codex`
- artifact: `/tank/projects/m80/docs/perf/ext4-overlay-template-clone.md`
- commit: `83be24e8610ab58b62e8d56fa8b66fcaa8aad0f0`
- git worktree dirty excluding this artifact: `true`
- substrate kind: `storage-only`
- command:

```sh
M80_RUN_EXT4_OVERLAY_TEMPLATE_CLONE=1 \
M80_EXT4_OVERLAY_TEMPLATE_SAMPLES=30 \
M80_EXT4_OVERLAY_TEMPLATE_RUN_ROOT=/var/tmp/m80-ext4-overlay-template-clone-codex \
M80_EXT4_OVERLAY_TEMPLATE_ARTIFACT=/tank/projects/m80/docs/perf/ext4-overlay-template-clone.md \
cargo test -p m80-storage --test ext4_overlay_template_clone -- --ignored --nocapture
```

This run primes the run-root-local empty overlay template once, then measures
`Rootfs::prepare` for N overlay clones. The base rootfs is a placeholder file
because `Rootfs::prepare` does not read or verify the base contents; callers own
base-image verification before this storage step.

## Overlay Template

- overlay size bytes: `67108864`
- template logical size bytes: `67108864`
- template allocated bytes after hole digging: `4440064`
- clone mode: `byte-copy fallback` (`ext4` is classified non-reflink by the runtime gate)

## Observable

- samples: `30`
- phase_3b_rootfs_prepare p50_ms: `7.006`
- phase_3b_rootfs_prepare p95_ms: `8.074`
- phase_3b_rootfs_prepare p99_ms: `8.389`
- phase_3b_rootfs_prepare min_ms: `6.722`
- phase_3b_rootfs_prepare max_ms: `8.389`
- reconsider dm-snapshot threshold: `p50 > 80 ms or p95 > 100 ms`
- threshold result: `keep byte-copy fallback`

## Device-Mapper Comparison

dm-snapshot was not prototyped in this run. The measured byte-copy fallback is
below the reconsider threshold, and adding a dm-snapshot prototype would touch
device-mapper setup/teardown, which the m80 audit-sweep doctrine treats as a
single-purpose kernel/device-mapper diff requiring its own real-KVM smoke if it
ever becomes justified.

- dmsetup devices before: `0`
- dmsetup devices after: `0`
- leaked dm devices: `0`

## Teardown Residue

- mount entries under run root before: `0`
- mount entries under run root after: `0`
- leaked mounts: `0`
- run root removed: `true`
