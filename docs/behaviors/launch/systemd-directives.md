# systemd VM Launch Directives

m80's systemd VM path is a transient `systemd-run` invocation, not an installed
`m80-vm@.service` template. The directive contract lives in
`crates/m80-firecracker/src/launch/systemd.rs` so per-VM values and static
hardening values are pinned in one Rust builder.

The VM unit sets the official jailer's bounding set, clears ambient
capabilities, sets `NoNewPrivileges=yes`, resets supplementary groups and
environment, uses `KeyringMode=private`, restricts address families to
`AF_UNIX AF_NETLINK AF_VSOCK`, enables the selected kernel-interface
protections, and mirrors `JailerConfig::resource_limits` into `Limit*`
properties. When m80 would previously request a private VMM network namespace,
the transient unit uses `PrivateNetwork=yes`. When the official jailer detaches
because `new_pid_ns` or `daemonize` is set, the transient unit uses
`Type=forking` plus `PIDFile=<jail-root>/<firecracker-basename>.pid` so systemd
tracks the Firecracker child rather than treating the exited jailer parent as a
completed service.

The builder does not emit an outer `SystemCallFilter=` in Phase 1. Firecracker
installs its own seccomp filter once it starts, and an outer jailer filter is
either too loose to matter or tight enough to break the official jailer.
It also does not emit `RestrictNamespaces=` because the official jailer must
create or join the mount, pid, and network namespaces that form the VM jail.
It does not emit `PrivateDevices=yes` because the official jailer creates the
device nodes that Firecracker needs inside the chroot.

Stdout and stderr go to `append:<console-log>` when `JailerConfig::stdio_log`
is present; otherwise both are `null`. The unit name is derived from a sha256
over the run directory and VM id, so caller-controlled basenames cannot create
invalid unit names or collide across parent directories.

## Evidence

- `crates/m80-firecracker/src/launch/systemd.rs`
- `crates/m80-jailer-harden/src/lib.rs::OFFICIAL_JAILER_CAPABILITIES`
- `docs/decisions/0010-systemd-launch-default.md`
