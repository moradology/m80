# `m80-jailer`

Per-VM jailer chroot materialization, replayable plan + state JSON, and
recovery from prior runs.

## Reason for being

Firecracker ships an official `jailer` binary that builds a chroot
sandbox before exec'ing `firecracker`. Sequestering the bind-plan +
materialize + recovery dance into one crate keeps the rest of the
codebase blissfully ignorant of the jailer protocol; the orchestrator
hands a config in and gets back a launchable chroot — or a typed error.

## Black-box contract

- `Plan::compute(config: &JailerConfig) -> Result<Plan, JailerError>` is
  pure — it does not touch the filesystem.
- `Plan::materialize(&self) -> Result<MaterializedJail, JailerError>`
  performs the filesystem mutations directly via syscalls + `Command::new`,
  relying on the m80 process's already-verified privilege. Steps are
  recorded to `jailer-state.json`; `Drop` tears the chroot down. Jail-root
  and in-jail directories are created `0700` and chowned to the configured
  jail uid/gid. Bind sources are canonicalized before use; file creation
  uses `O_NOFOLLOW`; bind mounts use `MS_BIND|MS_REC` and are remounted with
  `MS_NODEV|MS_NOEXEC|MS_NOSUID` (`MS_RDONLY` for read-only binds).
  Bind destinations under `dev`, `proc`, or `sys` are rejected: device nodes
  and virtual kernel filesystems are the official jailer's responsibility,
  not caller-provided host binds.
- The plan is replayable: `jailer-plan.json` reproduces the chroot
  offline for triage. Reproducibility is enforced by tests.
- `MaterializedJail::launch(...)` exec's `firecracker` inside the jail
  via `m80-jailer-harden` and Firecracker's official jailer binary. The
  hardening wrapper first applies m80's extended inherited resource limits
  (`nproc`, `memlock`, address-space, core, stack, plus mirrored `no-file` and
  `fsize`), optionally enters a private cgroup namespace, drops supplementary
  groups, clears inheritable/ambient capabilities, sets `no_new_privs`, sets
  `PDEATHSIG=SIGKILL`, resets the signal mask, and sets umask `0077`, then
  execs the official jailer. m80 still passes `--resource-limit no-file=<n>` on every launch, optionally passes
  `--resource-limit fsize=<bytes>` to Firecracker's official jailer, clears the
  jailer process environment, gives stdin `/dev/null`, gives stdout/stderr
  either the configured log file or `/dev/null`, can pass `--new-pid-ns`, can
  pass `--daemonize`, and can pass `--netns <path>` after validating the path
  with `O_NOFOLLOW` and `NSFS_MAGIC`.
  Without `new_pid_ns` or `daemonize`, jailer `exec()`s into firecracker, so
  `jailer_pid` and `firecracker_pid` refer to the same OS process. With
  `new_pid_ns` or `daemonize`, the official jailer writes the Firecracker PID
  file and exits; m80 reaps that parent and records `jailer_pid = 0` as the
  no-live-jailer sentinel.
- Mount namespace and `pivot_root` isolation, `/dev/{kvm,net/tun,urandom}`
  `mknod`, `/proc`/`/sys` omission, private copy of the Firecracker
  executable, startup environment clearing, and close-range hygiene are
  delegated to Firecracker's official jailer. m80 does not run `pivot_root` in
  `Plan::materialize()` because that code runs in the host orchestrator
  process; doing so would isolate the orchestrator instead of the Firecracker
  process.
- If `JailerConfig::stdio_log` is `Some(path)`, `launch` appends the
  jailed process stdout and stderr to that host file. m80-firecracker
  sets this to `<run_dir>/console.log` so Firecracker VMM output and the
  guest serial console survive launch failures and stopped-VM triage.
- `inspect_run_dir` reads any prior plan + state and returns
  `LiveJail | OrphanJail { reap_steps } | NoJail`. If a replayable
  `jailer-plan.json` exists but `jailer-state.json` is missing or
  malformed, recovery fails closed to `OrphanJail` with the plan steps
  reversed so callers can still unmount/reap partial materialization. The
  crate does not act on the decision; the caller does.
- The actual chroot path is `<run_dir>/<firecracker basename>/<run_dir basename>/root/`
  — jailer's hardcoded layout, derived in `jail_root_path()`. We pre-create
  the parent dirs and bind RW sources are chowned to `uid:gid` so the
  jailed firecracker can open them.
- This crate does **not** decide where the run_dir is — that's the
  orchestrator's job.

## Public surface

- `JailerConfig`, including `resource_limits`, `new_pid_ns`, `daemonize`,
  `new_cgroup_ns`, optional `netns_path`,
  `jailer_harden_bin`, optional `stdio_log`, `Binding { source, dest, mode }`,
  `BindMode { Ro, Rw, CreateInsideJail }`, `JailerSocket`.
- `ResourceLimits { no_file, fsize, nproc, memlock, address_space, core, stack }`;
  defaults are `no_file = 2048`, `fsize = None`, `nproc = None`,
  `memlock = 0`, `address_space = None`, `core = 0`, and `stack = 8 MiB`.
  Only `no_file` and `fsize` are forwarded to Firecracker's official jailer;
  the rest are applied by `m80-jailer-harden` before exec.
- `Plan`, `MaterializedJail`, `JailedFirecracker`.
- `jail_root_path(run_dir, firecracker_bin)` for pure layout computation.
- `inspect_run_dir`, `InspectionDecision`.
- `JailerError`: `BindFailed`, `ChrootFailed`, `FirecrackerPidTimeout`,
  `UidGidInvalid`, `InvalidNetns`, `Io { path, source }`. Privilege is verified once by
  `m80-preflight`; this crate does not run a per-launch sudo probe.

## Non-goals

- **No cgroup configuration.** `m80-cgroup` owns that.
- **No cgroup placement.** `new_cgroup_ns` hides the host hierarchy from the
  jailed process, but `m80-cgroup` still owns cgroup creation, controller
  limits, OOM scoring, and PID enrolment.
- **No network namespace creation.** JoinNetns callers provision namespaces;
  this crate only validates and passes the namespace fd to Firecracker's jailer.
- **No process supervision after launch.** The orchestrator owns the
  child PIDs once `launch()` returns.
- **No general "make me a chroot" service.** This crate is shaped around
  Firecracker's jailer specifically.

## Dependencies

- `serde`, `serde_json`, `thiserror`, `tracing`, `nix`.
- No other m80 crates.
- Requires the `jailer` binary on `PATH` (or a configured path) and
  privilege (`CAP_SYS_CHROOT` / root) at materialize time; `m80-preflight`
  verifies at startup.

## Tests

- `tests/plan_compute.rs` — pure `Plan::compute` produces expected
  bind-source paths, private jail-internal directory modes, rejected
  `/proc`/`/sys`/escaping destinations, and jail-root layout given fixed
  inputs; no filesystem access.
- `tests/recover.rs` — `inspect_run_dir` returns `NoJail` for an empty
  run-dir, `OrphanJail` for plan-only, partial-state, or stale-pid residue,
  and `LiveJail` when the state JSON records a running pid, including the
  `new_pid_ns` `jailer_pid = 0` sentinel.
- `tests/jail_root_path.rs` — `jail_root_path` output matches the
  expected jailer-hardcoded layout for several input combinations.
- Unit tests in `src/materialized.rs` — launch argument plumbing for
  the hardening wrapper, resource limits, environment clearing, stdio capture,
  netns validation, `new_pid_ns` parent reaping, and daemonized parent reaping.
- `tests/integration_root.rs` — ignored root-only smoke for real
  materialization and real Firecracker-jailer `--new-pid-ns` launch state
  (`jailer_pid = 0`, Firecracker `NSpid` ends in `1`, resource limit live,
  private Firecracker executable copy).
- `tests/defense_in_depth.rs` — ignored root-only harness that launches the
  musl `m80-attack-runner` payload through the official Firecracker jailer
  path and waits for its exit code. The `echo_zero` negative control must exit
  `0` so the harness can prove it detects a successful attack as a breach.
  The same harness pins the filesystem escape battery: dotdot/openat-style
  chroot escape attempts, proc-self-root escape, host sentinel read/write, and
  lower-layer write attempts must all exit non-zero. It also pins process/PID
  isolation: host PID status/cmdline/mountinfo observation, host-PID signal
  probes, and broad host process enumeration must all exit non-zero. Privilege
  escalation attempts to become uid/gid 0, retain capabilities, unshare/mount,
  or change host identity must all exit non-zero.
