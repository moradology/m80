# Guest Exec — Image Contents and PID-1 Boot

## daemon-binary

The Ubuntu image-build pipeline installs the guest daemon at
`/m80-guestd` inside the rootfs and marks it executable. The kernel command
line includes `init=/m80-guestd`, so Ubuntu and Minimal image kinds share the
same PID-1 startup path.

Source: dossier `03-guest-daemon.md` § Image contents; predecessor
`infra/firecracker/prepare-guestd-image.sh` lines 5, 205, and 219-220.

Test: `m80-image-build::pipeline::tests::installs_guest_daemon_binary_as_pid_one_target`.

## init-link

The Ubuntu image installs `/init` as a symlink to `/m80-guestd`. The host
currently uses `init=/m80-guestd` directly, but the symlink keeps the image
self-describing and matches the Minimal image layout. The image does not
install or enable `m80-guestd.service`.

Test: `m80-image-build::pipeline::tests::installs_init_symlink_for_pid_one_boot`.

## pid-one-mountpoints

The Ubuntu and Minimal images pre-create `/workspace`, `/proc`, `/sys`, `/dev`,
`/etc`, `/lower`, `/upper`, and `/merged`. `/lower`, `/upper`, and `/merged`
must exist before boot because the initial root is mounted read-only; PID-1
guestd uses them for the base+overlay pivot. `/etc` must exist before outbound
PID-1 network setup can write `/etc/resolv.conf` in the pivoted root.

Test: `m80-image-build::pipeline::tests::installs_pid_one_mountpoint_dirs`.

## no-systemd-units

The Ubuntu image kind no longer installs `m80-guestd.service` or
`workspace.mount`. Workspace mounting is handled by PID-1 guestd after the
overlay pivot, using `/dev/vdc` when `m80.workspace=1`.

Test: `m80-image-build::pipeline::tests::does_not_install_systemd_units_for_guestd_startup`.

## env-file

The Ubuntu image does not install `/etc/default/m80-guestd`, and
there is no service `EnvironmentFile`. Runtime readiness uses the fixed
inverted-ready vsock port and protocol byte; there is no host-injected
ready-marker override.

Source: dossier `03-guest-daemon.md` § Image contents; predecessor
`infra/firecracker/prepare-guestd-image.sh:7` and `guestd-rs.service:10`.
m80 intentionally hard-cuts over from that env-file behavior.

Test: `m80-image-build::pipeline::tests::does_not_install_guestd_environment_file`.
