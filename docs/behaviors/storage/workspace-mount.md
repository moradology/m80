# Behavior: Workspace Mount

**Bead:** `m80-6a0q.3.1`
**Date:** 2026-05-06
**Test:** `crates/m80-guestd/tests/storage/workspace_mount.rs`

## pid-one-workspace-device

In Minimal image PID-1 mode, `m80-guestd` mounts the optional workspace scratch
drive from `/dev/vdc` to `/workspace` only when the host boot cmdline contains
`m80.workspace=1`. `/dev/vdc` is the third Firecracker block device after the
storage pivot:

1. `/dev/vda` is the shared read-only base rootfs.
2. `/dev/vdb` is the per-VM writable rootfs overlay.
3. `/dev/vdc` is the optional workspace scratch image.

The old pre-overlay drive assumption, where the workspace drive occupied
`/dev/vdb`, is no longer valid. PID-1 must never mount `/dev/vdb` as the
workspace because that device is the overlay upper layer.

## after-pivot

The workspace mount runs after `mount_overlay_and_pivot` and after
`pivot_rootfs("/merged")`. This makes `/workspace` the mountpoint inside the
merged overlayfs root, not a mount on the discarded initial root.

## optional-no-drive

When no workspace was configured, Firecracker does not attach the workspace
drive and the boot cmdline contains `m80.workspace=0`. PID-1 skips the
workspace mount before inspecting `/dev/vdc`, because preallocated hotplug slots
may legitimately occupy that device position. Missing `/dev/vdc` remains
documented optional state when `m80.workspace=1`: guestd logs the skipped mount,
emits the `workspace_absent` boot milestone, and continues startup. Other mount
failures still fail PID-1 setup.
