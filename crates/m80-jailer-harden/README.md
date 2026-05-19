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

`m80-jailer-harden --jailer-bin <path> --uid <uid> --gid <gid> [--rlimit <name=value> ...] [--new-cgroup-ns] [--new-net-ns] -- <jailer-args...>`
does this, in order:

1. Optionally enter a private cgroup namespace with `unshare(CLONE_NEWCGROUP)`.
2. Optionally enter a private network namespace with `unshare(CLONE_NEWNET)`.
3. Apply requested inherited resource limits (`no-file`, `fsize`, `nproc`,
   `memlock`, `as`, `core`, `stack`) with equal soft/hard values.
4. Drop supplementary groups with `setgroups([])`.
5. Clear inheritable and ambient Linux capabilities.
6. Prune the bounding capability set to the official jailer minimum:
   `CAP_CHOWN`, `CAP_DAC_OVERRIDE`, `CAP_SYS_CHROOT`, `CAP_MKNOD`,
   `CAP_SETUID`, `CAP_SETGID`, and `CAP_SYS_ADMIN`.
7. Prune the effective and permitted capability sets so only currently-held
   members of that same official jailer minimum remain. Pre-jailer host caps
   such as `CAP_NET_ADMIN`, `CAP_KILL`, `CAP_FOWNER`, `CAP_SYS_PTRACE`,
   `CAP_SYS_MODULE`, `CAP_SYS_RAWIO`, and `CAP_SETPCAP` do not survive into
   the official jailer exec.
8. Set `PR_SET_NO_NEW_PRIVS`.
9. Set `PR_SET_PDEATHSIG` to `SIGKILL`.
10. Set umask to `0077`.
11. Reset the thread signal mask to empty.
12. Close inherited file descriptors above stdio with `close_range(3, UINT_MAX, 0)`.
13. Clear its environment and `exec` the official jailer with the remaining args.

The wrapper does not perform chroot, pivot_root, mknod, cgroup placement, setuid,
or setgid. Those stay owned by Firecracker's official jailer and `m80-cgroup`.

`m80-jailer-harden --version` prints `m80-jailer-harden <package-version>` and
exits before parsing hardening arguments or touching process state. The
install-time host-binaries manifest records that version string beside the
binary hash.

The default installed wrapper path is `/opt/m80/bin/m80-jailer-harden`.
`m80-preflight` and launch configuration may override that path with
`M80_JAILER_HARDEN_BIN`.

## Public Surface

| Public item | Contract |
| --- | --- |
| Binary `m80-jailer-harden` | Parses wrapper arguments, applies process hardening, clears the environment, and execs the official jailer. `--version` prints package identity without hardening side effects. |
| `HardenArgs` | Opaque parsed argument bundle returned by `parse_args` and consumed by `exec_jailer`. It carries the official jailer path, forwarded jailer args, requested `ResourceLimit` rows, and requested namespace flags; fields are crate-private. `--uid` and `--gid` remain required parse-time validation inputs but are not stored because the official jailer receives its own uid/gid through the forwarded args after `--`. |
| `HardenArgs::resource_limits()` | Read-only view of the parsed resource-limit rows for `apply_process_hardening`. |
| `HardenArgs::new_cgroup_ns()` | Parsed `--new-cgroup-ns` flag for `apply_process_hardening`. |
| `HardenArgs::new_net_ns()` | Parsed `--new-net-ns` flag for `apply_process_hardening`. |
| `ResourceLimit { kind, value }` | Public parsed resource-limit row used by tests and callers that inspect parse output. |
| `ResourceLimitKind` | Supported resource limit names: `NoFile`, `FSize`, `NProc`, `MemLock`, `AddressSpace`, `Core`, and `Stack`. |
| `HardenError` | Typed pre-exec failure surface: `MissingArgument`, `InvalidValue`, `MissingSeparator`, `MissingJailerArgs`, `SetGroups`, `ClearCaps` for capability pruning failures, `NoNewPrivs`, `ParentDeathSignal`, `CgroupNamespace`, `NetworkNamespace`, `SetResourceLimit`, `SignalMask`, `CloseRange`, and `Exec`. |
| `parse_args(args)` | Parses wrapper args from an iterator that starts after argv[0]; validates required `--jailer-bin`, `--uid`, `--gid`, separator, and forwarded jailer args. |
| `apply_process_hardening(resource_limits, new_cgroup_ns, new_net_ns)` | Applies the inheritable hardening sequence without execing. |
| `exec_jailer(args)` | Clears the environment and replaces the current process with the official jailer. Returns only if `exec` fails. |

## Non-Goals

- No fallback if hardening fails. A failed syscall aborts launch.
- No replacement for the official Firecracker jailer.
- No seccomp/AppArmor/SELinux policy installation.

## Dependencies

`anyhow`, `caps`, `m80-close-range`, `nix`, `thiserror`.

## Tests

- Unit tests cover CLI parsing, missing argument errors, and the official
  jailer capability allowlist so future audits see any drift.
- `tests/version.rs` proves `--version` returns the package version without
  entering the hardening path.
- Ignored root integration tests verify the wrapper's inherited process state
  via `/proc/self/status` including `CapBnd`, `CapPrm`, and `CapEff`,
  environment clearing, and inherited-fd closure inside the exec target. They
  also verify `--new-cgroup-ns` makes `/proc/self/cgroup` appear rooted at `/`
  after exec. Parser coverage pins `--new-net-ns` forwarding; real launch
  coverage for network namespace placement lives in `m80-firecracker`.
