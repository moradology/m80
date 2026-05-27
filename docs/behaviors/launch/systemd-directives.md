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
the transient unit uses `PrivateNetwork=yes`.

The builder does not emit an outer `SystemCallFilter=` in Phase 1. Firecracker
installs its own seccomp filter once it starts, and an outer jailer filter is
either too loose to matter or tight enough to break the official jailer.

Stdout and stderr go to `append:<console-log>` when `JailerConfig::stdio_log`
is present; otherwise both are `null`. The unit name is derived from a sha256
over the run directory and VM id, so caller-controlled basenames cannot create
invalid unit names or collide across parent directories.

## Evidence

- `crates/m80-firecracker/src/launch/systemd.rs`
- `crates/m80-jailer-harden/src/lib.rs::OFFICIAL_JAILER_CAPABILITIES`
- `docs/decisions/0010-systemd-launch-default.md`
