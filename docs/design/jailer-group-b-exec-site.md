# Jailer Group B Exec-Site Ownership

## Decision

Group B hardening that must run immediately before Firecracker's final `exec`
belongs at the process that performs that `exec`. In the current Phase 1
design, m80 applies inherited one-way hardening before Firecracker's official
`jailer`: by transient systemd unit on supported systemd hosts, and by
`m80-jailer-harden` on hosts without supported systemd. The final
`Command::exec()` still belongs to the official jailer.

m80 does not run these syscalls in `Plan::materialize()` or in the host
orchestrator process. That would harden the wrong process. Instead, the
systemd path applies unit-level state before starting the official jailer. The
wrapper path runs `m80-jailer-harden` as the spawned child, applies the subset
of one-way state that survives through the official jailer, and then execs the
official jailer.

## Current Phase 1 Scope

For Phase 1, m80 claims Firecracker-jailer parity only where the official
jailer already owns the behavior or exposes a stable flag:

- mount namespace, recursive slave propagation, `pivot_root`, and old-root
  detach are delegated to the official jailer;
- `/dev/kvm`, `/dev/net/tun`, `/dev/urandom`, and optional `/dev/userfaultfd`
  are created by the official jailer with `mknod`;
- `--resource-limit no-file=<n>` and optional `--resource-limit fsize=<bytes>`
  are passed through by m80;
- `--new-pid-ns` is passed through by m80, with the exited jailer parent reaped
  and represented as `jailer_pid = 0`;
- on systemd-selected hosts, the transient unit drops supplementary groups,
  clears ambient capabilities, applies the official jailer bounding set, sets
  `NoNewPrivileges=yes`, sets `UMask=0077`, uses `KeyringMode=private`, and
  applies the documented launch-unit hardening directives before starting the
  official jailer;
- on wrapper-selected hosts, `m80-jailer-harden` drops supplementary groups,
  clears inheritable and ambient capabilities, sets `PR_SET_NO_NEW_PRIVS`,
  sets `PR_SET_PDEATHSIG=SIGKILL`, sets umask `0077`, resets the signal mask,
  and closes inherited fds above stdio before execing the official jailer;
- m80 clears the wrapper environment before spawn on the wrapper path, the
  wrapper clears the environment again before execing the official jailer, the
  systemd path sets `Environment=`, and the official jailer clears its
  inherited environment again before dispatch;
- m80 pins stdin to `/dev/null` and pins stdout/stderr to the configured
  console log or `/dev/null`;
- the official jailer also closes inherited fds >= 3 with `close_range`.

## Not Claimed In Current Phase 1

The following Group B items remain final-exec-site hardening goals, but are not
owned directly by m80 while the official jailer performs the final exec:

- owning an explicit m80 `capset()` call after the jailer finishes privileged
  setup. Phase 1 verifies the live jailed Firecracker process has zero
  permitted/effective capabilities after the official jailer's uid/gid drop,
  but that drop is still delegated to the official jailer;
- proving `PR_SET_PDEATHSIG` survives the official jailer's final uid/gid
  transition on every supported kernel;
- installing any seccomp/AppArmor/SELinux policy at the final exec site.

## Rejected Paths

- **Run these syscalls in `Plan::materialize()`.** Rejected because it runs in
  the m80 orchestrator process before the official jailer exists.
- **Clear effective/permitted capabilities before execing the official jailer.**
  Rejected because the jailer still needs privilege for chroot/pivot/mknod.
- **Use raw `pre_exec`/FFI in `m80-jailer`.** Rejected because the workspace
  forbids unsafe code outside explicit safe-wrapper crates, and even a safe
  wrapper would still run before the official jailer's privileged setup.

## Next Implementable Shapes

There are two viable future paths:

1. **Patch or upstream Firecracker's official jailer** to add the missing final
   exec-site hardening, then require that jailer version in m80-preflight.
2. **Build an m80-owned launcher** with a dedicated safe syscall-wrapper crate
   and move the final exec site into m80. That is a larger architectural
   cutover and must include root/KVM proofs before replacing the official
   jailer path.

Until one of those paths is chosen, claims about Group B should distinguish
between inherited launch-path hardening, which Phase 1 applies and tests, and
final-exec-site capability/seccomp hardening, which remains a future shape.
