# Network Helper systemd Launch

Behavior capture for bead `m80-9wm35.6`.

`m80-firecracker` launches the process-global `m80-net-helper` according to
`m80_preflight::Discovery::chosen_launch_path`.

- `LaunchPath::Systemd`: launch one long-lived transient `systemd-run` unit for
  the backend helper lifetime, then send the existing newline-delimited JSON
  helper protocol over the unit stdio pipe.
- `LaunchPath::Wrapper`: launch the helper directly before the backend drops
  `CAP_NET_ADMIN`.

The helper protocol, request enum, response enum, and error framing do not
change between launch paths. Backend construction fails closed if a live
process-global helper already exists for a different helper binary or launch
mode.

The systemd helper unit pins this envelope:

- `CapabilityBoundingSet=CAP_NET_ADMIN CAP_SYS_ADMIN`
- `AmbientCapabilities=CAP_NET_ADMIN CAP_SYS_ADMIN`
- `NoNewPrivileges=yes`
- `KeyringMode=private`
- `RestrictSUIDSGID=yes`
- `RestrictAddressFamilies=AF_UNIX AF_NETLINK AF_INET`
- `RestrictNamespaces=net`
- `LockPersonality=yes`
- `SystemCallArchitectures=native`
- `SystemCallFilter=@system-service @network-io @mount`

`CAP_SYS_ADMIN` and `@mount` are present because named network namespaces are
published as bind mounts under `/run/netns`. The helper unit must not use
filesystem protection directives that move it into a private mount namespace:
the official jailer runs in a separate launch unit and must be able to join the
helper-created namespace through the host-visible `/run/netns/<name>` path.

Regression coverage:

- `crates/m80-firecracker/src/network_helper/systemd.rs::tests::net_helper_directive_snapshot_is_pinned`
- `crates/m80-firecracker/src/network_helper/tests.rs::process_global_helper_rejects_launch_mode_switch_for_same_binary`
- `crates/m80-firecracker/src/network_helper/tests.rs::systemd_helper_process_state_is_pinned`
  (ignored; requires root and systemd; also proves the helper-created netns is
  joinable from the host)
- `crates/m80-firecracker/tests/egress_outbound_real_kvm.rs::allow_outbound_firecracker_runs_in_private_netns`
  on both systemd and wrapper launch paths
