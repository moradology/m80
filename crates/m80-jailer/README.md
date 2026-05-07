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
  recorded to `jailer-state.json`; `Drop` tears the chroot down.
- The plan is replayable: `jailer-plan.json` reproduces the chroot
  offline for triage. Reproducibility is enforced by tests.
- `MaterializedJail::launch(...)` exec's `firecracker` inside the jail
  via the jailer binary (no `--daemonize`). Two PIDs are tracked: the
  `jailer_pid` comes from the spawned `Child` handle; `firecracker_pid`
  is read from `<jail_root>/firecracker.pid` after the jailer writes it.
  Because jailer `exec()`s into firecracker, `jailer_pid` and
  `firecracker_pid` end up referring to the same OS process — but both
  are captured separately for state persistence and recovery.
- If `JailerConfig::stdio_log` is `Some(path)`, `launch` appends the
  jailed process stdout and stderr to that host file. m80-firecracker
  sets this to `<run_dir>/console.log` so Firecracker VMM output and the
  guest serial console survive launch failures and stopped-VM triage.
- `recover_from_run_dir` reads any prior plan + state and returns
  `LiveJail | OrphanJail { reap_steps } | NoJail`. The crate does not
  act on the decision; the caller does.
- The actual chroot path is `<run_dir>/<firecracker basename>/<run_dir basename>/root/`
  — jailer's hardcoded layout, derived in `jail_root_path()`. We pre-create
  the parent dirs and bind RW sources are chowned to `uid:gid` so the
  jailed firecracker can open them.
- This crate does **not** decide where the run_dir is — that's the
  orchestrator's job.

## Public surface

- `JailerConfig`, including optional `stdio_log`, `Binding { source, dest, mode }`,
  `BindMode { Ro, Rw, CreateInsideJail }`, `SocketSpec`.
- `Plan`, `MaterializedJail`, `JailedFirecracker`.
- `jail_root_path(run_dir, firecracker_bin)` for pure layout computation.
- `recover_from_run_dir`, `RecoveryDecision`.
- `JailerError`: `BindFailed`, `ChrootFailed`, `FirecrackerPidTimeout`,
  `UidGidInvalid`, `Io { path, source }`. Privilege is verified once by
  `m80-preflight`; this crate does not run a per-launch sudo probe.

## Non-goals

- **No cgroup configuration.** `m80-cgroup` owns that.
- **No network namespace setup.** `m80-net-outbound` lives outside.
- **No process supervision after launch.** The orchestrator owns the
  child PIDs once `launch()` returns.
- **No general "make me a chroot" service.** This crate is shaped around
  Firecracker's jailer specifically.

## Dependencies

- `serde`, `serde_json`, `thiserror`, `tracing`.
- No other m80 crates.
- Requires the `jailer` binary on `PATH` (or a configured path) and
  privilege (`CAP_SYS_CHROOT` / root) at materialize time; `m80-preflight`
  verifies at startup.

## Tests

- `tests/plan_compute.rs` — pure `Plan::compute` produces expected
  bind-source paths and jail-root layout given fixed inputs; no filesystem
  access.
- `tests/recovery.rs` — `recover_from_run_dir` returns `NoJail` for a
  missing run-dir, `OrphanJail` for a plan-only (no live pid) dir, and
  `LiveJail` when the state JSON records a running pid.
- `tests/jail_root_path.rs` — `jail_root_path` output matches the
  expected jailer-hardcoded layout for several input combinations.
