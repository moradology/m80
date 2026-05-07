# Jailer Security Parity

`m80-jailer` delegates process-side jail entry to Firecracker's official
`jailer` binary. m80 materializes the host-side bind plan, then launches the
official jailer with explicit arguments. The official jailer owns the fresh
mount namespace, recursive slave propagation, `pivot_root`, old-root detach,
in-jail device-node creation, PID namespace entry, environment cleanup, and
close-range cleanup before Firecracker is exec'd.

`Plan::materialize()` does not call `unshare()` or `pivot_root()`: it runs in
the host orchestrator process, so doing that there would isolate m80 itself
rather than the Firecracker process. The m80-owned hardening surface is the
materialized bind plan: jail-internal directories are `0700` and chowned to
the jail uid/gid, bind sources are canonicalized before use, placeholder and
state-file writes use `O_NOFOLLOW`, binds are recursive, and bind remounts add
`MS_NODEV`, `MS_NOEXEC`, and `MS_NOSUID` with `MS_RDONLY` for read-only binds.

`JailerConfig::resource_limits` is persisted in `jailer-plan.json` and passed
as Firecracker-jailer `--resource-limit` arguments. The default is
`no-file=2048`, matching Firecracker's jailer default, with optional `fsize`.
m80 launches the official jailer through `m80-jailer-harden`. The wrapper drops
supplementary groups, clears inheritable and ambient capabilities, sets
`PR_SET_NO_NEW_PRIVS`, sets `PR_SET_PDEATHSIG` to `SIGKILL`, resets the signal
mask, sets umask `0077`, closes inherited fds above stdio, clears its
environment, and then execs the official jailer. m80 also pins stdio: stdin is
`/dev/null`; stdout/stderr are the
configured console log when present and `/dev/null` otherwise.

`JailerConfig::new_pid_ns` maps directly to Firecracker-jailer
`--new-pid-ns`. When disabled, the jailer process execs Firecracker and
`jailer_pid == firecracker_pid`. When enabled, the jailer parent writes
`firecracker.pid`, exits, and m80 records `jailer_pid = 0` as the
no-live-jailer sentinel while tracking the live Firecracker PID normally.

m80 rejects bind destinations that are absolute, empty, contain `..`, or start
with `dev`, `proc`, or `sys`; the jail must not expose host device nodes,
`/proc`, or `/sys` through caller-provided binds.

Runtime evidence:

- `crates/m80-firecracker/tests/end_to_end_real_kvm.rs::end_to_end_real_kvm_jailer_security_parity`
  boots a real VM, then inspects the live Firecracker process for a distinct
  mount namespace, `RLIMIT_NOFILE`, jail uid/gid, empty supplementary groups,
  `NoNewPrivs: 1`, zero permitted/effective/inheritable/ambient caps, empty
  signal mask, hardened bind mount flags, read-only rootfs binding, and
  jailer-created `/dev/kvm`, `/dev/net/tun`, and `/dev/urandom` character
  devices.
- `crates/m80-jailer/tests/integration_root.rs::launch_with_new_pid_ns_records_sentinel_and_firecracker_is_pid_one`
  launches real Firecracker through the official jailer with `--new-pid-ns`,
  then asserts `jailer_pid = 0`, Firecracker's `NSpid` ends in `1`, and the
  configured resource limit and inherited hardening state are live.
- `crates/m80-jailer-harden/tests/integration_root.rs::wrapper_applies_inherited_hardening_before_exec`
  execs a shell through the hardening wrapper and inspects `/proc/self/status`,
  the environment, and a deliberately inherited fd for the wrapper-level
  inherited process state.
