# Storage — Overlay Assembly Across Image Kinds

`ImageKind::Ubuntu` and `ImageKind::Minimal` both boot `m80-guestd` as PID 1.
The image kind selects the userland rootfs family, not a different startup
model. The host therefore uses one drive order for both kinds:

1. read-only base rootfs at `/dev/vda`;
2. per-VM writable rootfs overlay at `/dev/vdb`;
3. optional workspace scratch drive at `/dev/vdc`.

At boot, PID-1 guestd mounts `/dev/vda` as the lower layer, `/dev/vdb` as the
upper/work layer, mounts overlayfs as `/`, pivots into the merged root, and
then mounts `/dev/vdc` at `/workspace` when `m80.workspace=1`.

The Ubuntu build path installs `/m80-guestd`, `/init -> /m80-guestd`, and the
same `/lower`, `/upper`, `/merged`, and `/workspace` mountpoint contract as the
Minimal build path. It does not install `m80-guestd.service` or
`workspace.mount`; old schema-v3 Ubuntu artifacts must be rebuilt because
schema v4 is the first manifest version for this hard cutover.

Test:
`crates/m80-firecracker/tests/image_kind_dispatch_real_kvm.rs::overlay_assembly_per_image_kind`.
