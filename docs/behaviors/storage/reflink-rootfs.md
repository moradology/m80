# Reflink Rootfs Overlay Clone

## What

`Rootfs::prepare` keeps the base rootfs shared and read-only. It only creates a
per-VM writable overlay by cloning a run-root-local empty ext4 template to the
VM's overlay path.

The clone path is explicit:

- Reflink-capable filesystems use `cp --reflink=always --sparse=auto`.
- Filesystems classified as non-reflink use `cp --reflink=never --sparse=always`.
- Probe failures also use `cp --reflink=never --sparse=always`; launch remains
  available and the event tells the operator why reflinks are not in use.

The byte-copy path is permanent. ext4 production hosts are valid even though
they do not get metadata-only overlay clones.

## Phase G Recheck: dm-snapshot on ext4

`m80-q420k.8.7` revisited whether ext4 hosts should use dm-snapshot instead
of the current fallback. The current fallback is not the old full base-rootfs
copy: `Rootfs::prepare` shares the base rootfs read-only and byte-copies only
the run-root-local empty sparse overlay template when the run-root filesystem
does not support reflinks.

Cold-launch data shows `phase_3b_rootfs_prepare` around 13-14 ms P50 after the
overlay-template path. The storage-only ext4 run in
`docs/perf/ext4-overlay-template-clone.md` isolates the fallback further: after
priming the empty overlay template, N=30 `Rootfs::prepare` clones measured
single-digit milliseconds on the representative ext4 run root. That is below
the 80 ms P50 / 100 ms P95 reconsider threshold and does not justify adding
device-mapper lifecycle state to the launch path.

dm-snapshot also changes kernel-facing behavior: it introduces per-VM
device-mapper setup, teardown, and leak modes that would require a
single-purpose kernel-touching diff with real-KVM smoke evidence. Reconsider it
only if a future committed ext4 artifact shows `phase_3b_rootfs_prepare` above
80 ms P50 or 100 ms P95, or if operator evidence shows byte-copy residue or
pressure that the current path cannot handle.

## Probe

`m80-storage` probes per run-root device and memoizes the result for the process
lifetime. The probe first calls `statfs`:

- XFS, Btrfs, ZFS, and unknown filesystem kinds run a same-directory safe
  FICLONE probe through `rustix::fs::ioctl_ficlone`.
- ext2, ext3, ext4, tmpfs, overlayfs, and FUSE short-circuit to non-reflink.

The memoization key is `st_dev`, so VMs under the same run-root device share
one decision.

## Observability

The runtime clone gate emits `m80_storage::rootfs` events:

- Unsupported or probe-failed devices emit one `info` event per device per
  process with `mode="byte_copy"`.
- A device that probed as supported but rejects `cp --reflink=always` at clone
  time emits one `warn` for that call with `fallback="byte_copy"` and then
  retries with `cp --reflink=never`.

Operators can grep the rootfs target to confirm whether a deployment is getting
reflink clones or explicit byte-copy fallback.

## What This Is Not

This is not a public API. `Rootfs::prepare` retains the same signature and
returns the same `Rootfs` paths.

This is not the `m80-preflight` advisory. Preflight reports whether the run-root
appears reflink-capable before launch; the runtime gate is the source of truth
used for each overlay-template clone.

## Tests

- `crates/m80-storage/tests/reflink_probe.rs::tmpfs_probe_reports_known_unsupported_without_ficlone`
- `crates/m80-storage/tests/reflink_probe.rs::cached_probe_returns_same_result_for_same_device`
- `crates/m80-storage/tests/reflink_rootfs.rs::concurrent_fill_produces_eight_independent_overlays`
- `crates/m80-storage/tests/reflink_rootfs.rs::concurrent_fill_does_not_double_emit_byte_copy_event`
- `crates/m80-storage/tests/ext4_overlay_template_clone.rs::ext4_overlay_template_clone_measurement` (ignored measurement harness)

Related docs:

- `crates/m80-storage/README.md`
- `docs/ops/host-tuning.md`
- `docs/behaviors/preflight/run-root-filesystem-advisory.md`
