# `m80-jailer-harden`

Small exec wrapper that applies process hardening which safely survives into
Firecracker's official jailer, then `exec`s that jailer.

## Reason For Being

Some Group B hardening is inheritable and one-way: supplementary groups,
ambient capabilities, `no_new_privs`, signal mask, and umask can be fixed before
the official jailer starts without interfering with its mount, chroot, mknod,
resource-limit, and PID-namespace setup. Keeping this in a binary gives m80 a
real process boundary to test without putting raw FFI into `m80-jailer`.

## Black-Box Contract

`m80-jailer-harden --jailer-bin <path> --uid <uid> --gid <gid> -- <jailer-args...>`
does this, in order:

1. Drop supplementary groups with `setgroups([])`.
2. Clear inheritable and ambient Linux capabilities.
3. Set `PR_SET_NO_NEW_PRIVS`.
4. Set `PR_SET_PDEATHSIG` to `SIGKILL`.
5. Set umask to `0077`.
6. Reset the thread signal mask to empty.
7. Close inherited file descriptors above stdio.
8. Clear its environment and `exec` the official jailer with the remaining args.

The wrapper does not perform chroot, pivot_root, mknod, cgroup setup, setuid, or
setgid. Those stay owned by Firecracker's official jailer.

## Public Surface

- Binary: `m80-jailer-harden`.
- Library helpers used by tests: `parse_args`, `apply_process_hardening`,
  `exec_jailer`, `run`.

## Non-Goals

- No fallback if hardening fails. A failed syscall aborts launch.
- No replacement for the official Firecracker jailer.
- No seccomp/AppArmor/SELinux policy installation.

## Dependencies

`anyhow`, `caps`, `nix`, `thiserror`.

## Tests

- Unit tests cover CLI parsing and missing argument errors.
- Ignored root integration tests verify the wrapper's inherited process state
  via `/proc/self/status`, environment clearing, and inherited-fd closure inside
  the exec target.
