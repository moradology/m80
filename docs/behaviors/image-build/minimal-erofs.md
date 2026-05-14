# Minimal Erofs Image Build

Bead: `m80-jp6ik.8`

`m80-image-build` accepts `rootfs.kind = "minimal-erofs"` for the same busybox
+ static `m80-guestd` userland as `kind = "minimal"`, but emits a compressed
read-only erofs base image at `output.erofs`.

The manifest stays `image_kind = Minimal` because the userland and startup
model are unchanged. The filesystem distinction is recorded separately as
`rootfs_format = Erofs`. Host launch planning turns that manifest field into
`m80.rootfs=erofs rootfstype=erofs` boot args. PID-1 guestd reads
`m80.rootfs=<ext4|erofs>` and mounts `/dev/vda` at `/lower` with the declared
filesystem, failing closed if the token is missing or unknown.

The stripped-kernel config keeps erofs built in (`CONFIG_EROFS_FS=y` plus LZ4
compression support) so erofs images can be used with the same no-initrd boot
path as ext4 images. The stock Firecracker kernel used in the current smoke
environment does not mount the erofs base rootfs, so `scripts/smoke.sh`
defaults `M80_IMAGE_KIND=minimal-erofs` to the stripped kernel path when
`M80_KERNEL_KIND` is not set.

Performance evidence is captured in `docs/perf/minimal-erofs.md`: the image is
far smaller and boots, but the measured `phase_12b_ready_accept` P50 reduction
missed the bead's latency acceptance threshold.

Tests:

- `crates/m80-image-build/tests/dry_run_smoke.rs::minimal_erofs_dry_run_prints_erofs_steps_and_creates_no_output_files`
- `crates/m80-image-manifest/tests/manifest_boot_fields.rs::records_rootfs_format`
- `crates/m80-firecracker/src/preboot_boot_arg_tests.rs::boot_args_mark_erofs_rootfs`
- `crates/m80-guestd/src/pid_one/tests.rs::rootfs_cmdline_token_selects_base_mount_fstype`
- `crates/m80-firecracker/tests/image_kind_dispatch_real_kvm.rs::image_kind_minimal_erofs_boots`
