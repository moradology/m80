# Guest SUID/SGID Suppression

Ubuntu image builds extract the upstream squashfs with `unsquashfs -no-xattrs`
and clear SUID/SGID mode bits from the extracted tree before `mkfs.ext4`.
This prevents upstream package file capabilities and SUID helpers from entering
the m80 Ubuntu base image.

At boot, PID-1 mounts guest-writable storage with `MS_NOSUID | MS_NODEV`:

- `/dev/vdb` at `/upper`, used as the overlayfs upper/work backing store;
- the overlayfs root at `/merged`;
- optional workspace ext4 at `/workspace`.

The mounts intentionally do not use `MS_NOEXEC`; ordinary workloads still need
to execute files they place in the writable rootfs or workspace. The security
property is privilege suppression, not execution suppression.

Workspace writeback also strips privilege mode bits before publishing extracted
files back onto the host. `Scratch::extract` preserves ordinary `0o777`
permissions but clears setuid, setgid, and sticky bits from files and
directories in the staging tree.

Tests:

- `crates/m80-image-build/src/pipeline/tests.rs::unsquashfs_command_disables_xattr_extraction`
- `crates/m80-image-build/src/pipeline/tests.rs::strip_suid_sgid_bits_clears_tree_without_following_symlinks`
- `crates/m80-guestd/src/pid_one/tests.rs::writable_rootfs_overlay_mounts_disable_suid_and_devices`
- `crates/m80-guestd/src/pid_one/tests.rs::workspace_mount_disables_suid_and_devices`
- `crates/m80-storage/src/scratch.rs::tests::build_stage_strips_setuid_bits_from_extracted_file`
- `crates/m80-storage/src/scratch.rs::tests::build_stage_strips_setgid_bits_from_extracted_dir`
