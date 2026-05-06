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

The mounted rootfs receives `m80-guestd` at `/usr/local/bin/m80-guestd` with
mode `0755`.

Source: dossier `03-guest-daemon.md` § Image contents; predecessor
`infra/firecracker/prepare-guestd-image.sh` lines 5, 205, and 219-220.

Test: `m80-image-build::pipeline::tests::installs_guest_daemon_binary_at_usr_local_bin`.

## systemd-units

The mounted rootfs receives `m80-guestd.service` and `workspace.mount` under
`/etc/systemd/system/`. `m80-guestd.service` is enabled in
`basic.target.wants`; `workspace.mount` is enabled in
`multi-user.target.wants`. m80 does not install interpreter packages and does
not inspect Python, Node, npm, pip, or other tool runtimes during image build.

Source: dossier `03-guest-daemon.md` § Image contents; predecessor
`infra/firecracker/guestd-rs.service` and
`infra/firecracker/var-lib-predecessor-workspace.mount`.

Tests: `m80-image-build::pipeline::tests::installs_service_unit_for_basic_target_boot`,
`m80-image-build::pipeline::tests::installs_workspace_mount_unit`, and
`m80-image-build/tests/dry_run_smoke.rs::dry_run_prints_steps_to_stderr_and_creates_no_output_files`.

## workspace-directory

The mounted rootfs contains `/workspace` so the workspace mount unit has a
stable target directory. The guest daemon is not ordered after that mount;
workspace availability is observed by the child process at exec time.

Source: dossier `03-guest-daemon.md` § Image contents.

Test: `m80-image-build::pipeline::tests::installs_workspace_mount_unit`.
