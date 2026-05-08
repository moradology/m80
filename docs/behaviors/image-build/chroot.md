# Image Build — Ubuntu Rootfs Customization

## loop-mount

The Ubuntu pipeline loop-mounts the output ext4 rootfs read-write before
installing guest files and unmounts it before hashing artifacts or writing the
manifest. The mount and unmount operations fail closed with phase context.

Source: predecessor `infra/firecracker/prepare-guestd-image.sh` rootfs
customization stage.

Test: `m80-image-build/tests/dry_run_smoke.rs::dry_run_prints_steps_to_stderr_and_creates_no_output_files`
pins the mount/install/unmount step order without requiring root.

## daemon-binary

The mounted rootfs receives `m80-guestd` at `/m80-guestd` with mode `0755`.

Source: dossier `03-guest-daemon.md` § Image contents; predecessor
`infra/firecracker/prepare-guestd-image.sh` lines 5, 205, and 219-220.

Test: `m80-image-build::pipeline::tests::installs_guest_daemon_binary_as_pid_one_target`.

## pid-one-layout

The mounted rootfs receives `/init -> /m80-guestd` and the PID-1 mountpoint
dirs `/workspace`, `/proc`, `/sys`, `/dev`, `/lower`, `/upper`, and `/merged`.
m80 does not install interpreter packages and does not inspect Python, Node,
npm, pip, or other tool runtimes during image build.

Source: dossier `03-guest-daemon.md` § Image contents; predecessor
`infra/firecracker/guestd-rs.service` and
`infra/firecracker/var-lib-predecessor-workspace.mount`.

Tests: `m80-image-build::pipeline::tests::installs_init_symlink_for_pid_one_boot`,
`m80-image-build::pipeline::tests::installs_pid_one_mountpoint_dirs`, and
`m80-image-build/tests/dry_run_smoke.rs::dry_run_prints_steps_to_stderr_and_creates_no_output_files`.

## no-systemd-units

The mounted rootfs no longer receives `m80-guestd.service` or
`workspace.mount`. Guestd is PID 1 for Ubuntu and Minimal images, and
workspace availability is mounted by guestd after the overlay pivot.

Source: dossier `03-guest-daemon.md` § Image contents.

Test: `m80-image-build::pipeline::tests::does_not_install_systemd_units_for_guestd_startup`.
