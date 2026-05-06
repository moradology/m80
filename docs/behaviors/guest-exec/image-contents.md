# Guest Exec — Image Contents and Systemd Boot

## daemon-binary

The Ubuntu image-build pipeline installs the guest daemon at
`/usr/local/bin/m80-guestd` inside the rootfs and marks it executable. The
systemd service unit starts that exact path.

Source: dossier `03-guest-daemon.md` § Image contents; predecessor
`infra/firecracker/prepare-guestd-image.sh` lines 5, 205, and 219-220.

Test: `m80-image-build::pipeline::tests::installs_guest_daemon_binary_at_usr_local_bin`.

## systemd-unit

The Ubuntu image installs `/etc/systemd/system/m80-guestd.service` with
`Type=simple`, `DefaultDependencies=no`, `StandardOutput=journal+console`,
`StandardError=journal+console`, and `Restart=on-failure`. The unit is enabled
through `basic.target.wants`, not `multi-user.target.wants`, so guestd starts
before the network-wait-online path can delay cold launch.

Source: dossier `03-guest-daemon.md` § Image contents; predecessor
`infra/firecracker/guestd-rs.service` lines 7-13. m80 intentionally hard-cuts
over from the old `multi-user.target` boot path after the ubuntu cold-launch
flake described in `AGENTS.md`.

Test: `m80-image-build::pipeline::tests::installs_service_unit_for_basic_target_boot`.

## workspace-mount

The Ubuntu image installs `/etc/systemd/system/workspace.mount`, which mounts
`/dev/vdb` as ext4 at `/workspace`. The mount unit is enabled in
`multi-user.target.wants`. `m80-guestd.service` does not require or order after
`workspace.mount`; workspace availability is a per-exec filesystem fact, not a
daemon boot gate.

Source: dossier `03-guest-daemon.md` § Image contents; predecessor
`infra/firecracker/var-lib-predecessor-workspace.mount` lines 4-8 and
`guestd-rs.service` lines 5-6. m80 removes the daemon's unit dependency on the
mount to avoid the systemd ordering cycle documented in the service asset.

Test: `m80-image-build::pipeline::tests::installs_workspace_mount_unit`.

## env-file

The Ubuntu image does not install `/etc/default/m80-guestd`, and
`m80-guestd.service` does not consume an `EnvironmentFile`. Runtime readiness
uses the fixed inverted-ready vsock port and protocol byte; there is no
host-injected ready-marker override in the service environment.

Source: dossier `03-guest-daemon.md` § Image contents; predecessor
`infra/firecracker/prepare-guestd-image.sh:7` and `guestd-rs.service:10`.
m80 intentionally hard-cuts over from that env-file behavior.

Test: `m80-image-build::pipeline::tests::does_not_install_guestd_environment_file`.
