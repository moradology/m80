# Reflink Rootfs Overlay Clone

## What

`Rootfs::prepare` keeps the base rootfs shared and read-only. It only creates a
per-VM writable overlay by cloning a run-root-local empty ext4 template to the
VM's overlay path.

The clone path is explicit:

- `OverlayTemplateCloneMode::Reflink` uses
  `cp --reflink=always --sparse=auto`.
- `OverlayTemplateCloneMode::ByteCopy` uses
  `cp --reflink=never --sparse=auto`.
- `OverlayTemplateCloneMode::Auto` probes the run-root filesystem once and
  selects either `Reflink` or `ByteCopy` before the clone command is built.
  Probe failure is a hard error.

The byte-copy path is permanent. ext4 production hosts are valid even though
they do not get metadata-only overlay clones.

## Phase G Recheck: dm-snapshot on ext4

`m80-q420k.8.7` revisited whether ext4 hosts should use dm-snapshot instead
of explicit byte-copy. Explicit byte-copy is not the old full base-rootfs
copy: `Rootfs::prepare` shares the base rootfs read-only and byte-copies only
the run-root-local empty sparse overlay template when the run-root filesystem
or caller policy does not use reflinks.

Cold-launch data shows `phase_3b_rootfs_prepare` around 13-14 ms P50 after the
overlay-template path. The storage-only ext4 run in
`docs/perf/ext4-overlay-template-clone.md` isolates byte-copy further: after
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

`m80-storage` probes only when the caller selects `OverlayTemplateCloneMode::Auto`.
The result is memoized per run-root device for the process lifetime. The probe
first calls `statfs`:

- XFS, Btrfs, ZFS, and unknown filesystem kinds run a same-directory safe
  FICLONE probe through `rustix::fs::ioctl_ficlone`.
- ext2, ext3, ext4, tmpfs, overlayfs, and FUSE short-circuit to non-reflink.

The memoization key is `st_dev`, so VMs under the same run-root device share
one decision.

## Observability

The selected policy is visible at the caller boundary:

- Rust callers set `SandboxConfig::overlay_clone_mode` or pass
  `OverlayTemplateCloneMode` directly to `Rootfs::prepare`.
- CLI callers use `m80 run --overlay-clone-mode byte-copy|reflink|auto`.
- BootSpec files set `sandbox.overlay_clone_mode`.

Clone failure is reported as the selected `cp` failure. There is no hidden
runtime fallback event because m80 no longer retries another mode.

## What This Is Not

This is a public API. `Rootfs::prepare` takes an explicit
`OverlayTemplateCloneMode`, and `SandboxConfig` carries the same policy.

This is not the `m80-preflight` advisory. Preflight reports whether the run-root
appears reflink-capable before launch; explicit policy remains the source of
truth used for each overlay-template clone.

## Tests

- `crates/m80-storage/tests/reflink_probe.rs::tmpfs_probe_reports_known_unsupported_without_ficlone`
- `crates/m80-storage/tests/reflink_probe.rs::cached_probe_returns_same_result_for_same_device`
- `crates/m80-storage/tests/reflink_rootfs.rs::concurrent_fill_produces_eight_independent_overlays`
- `crates/m80-storage/tests/rootfs_prepare.rs::explicit_reflink_fails_closed_on_non_reflink_fs`
- `crates/m80-storage/tests/rootfs_prepare.rs::auto_selects_byte_copy_on_non_reflink_fs`
- `crates/m80-storage/tests/ext4_overlay_template_clone.rs::ext4_overlay_template_clone_measurement` (ignored measurement harness)

Related docs:

- `crates/m80-storage/README.md`
- `docs/ops/host-tuning.md`
- `docs/behaviors/preflight/run-root-filesystem-advisory.md`
