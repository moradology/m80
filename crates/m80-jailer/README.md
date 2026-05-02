# `m80-jailer`

Per-VM jailer chroot materialization, replayable plan + state JSON, and
recovery from prior runs.

## Reason for being

Firecracker ships an official `jailer` binary that builds a chroot
sandbox before exec'ing `firecracker`. The predecessor implementation is
~1,400 LOC because the chroot involves planning bind mounts (RO vs RW),
preparing socket paths, dropping uid/gid, and persisting enough state on
disk for recovery after a crash.

Sequestering all of that in `m80-jailer` keeps the rest of the
codebase blissfully ignorant of the jailer protocol. The orchestrator
hands `m80-jailer` a config and gets back a launchable command — or a
typed error.

A second reason: the jailer "plan" is a JSON document that's also
useful for triage, because it captures exactly which bind mounts were
attempted with which permissions. Owning the plan in this crate keeps
the schema stable.

## Black-box contract

- `Plan::compute(config: &JailerConfig) -> Result<Plan, JailerError>` is
  pure — it does not touch the filesystem. The result is a
  `serde`-serializable description of every bind, every directory
  creation, and every uid/gid drop.
- `Plan::materialize(&self) -> Result<MaterializedJail, JailerError>`
  performs the filesystem mutations directly via syscalls and `Command::new`,
  relying on the m80 process's already-verified privilege (root or
  `CAP_SYS_ADMIN`/etc.; see `m80-preflight`). Every step the handle takes
  is recorded to `jailer-state.json` in the run-dir; `Drop` tears the
  chroot down.
- The plan is replayable: dumping `jailer-plan.json` is enough to
  reproduce the chroot offline for triage. Reproducibility is enforced
  by tests.
- `MaterializedJail::launch(...)` exec's `firecracker` inside the jail
  and returns a `JailedFirecracker` carrying both the jailer pid and
  the firecracker child pid. Both are tracked separately because both
  must be reaped on teardown.
- `recover_from_run_dir(run_dir: &Path) -> Result<RecoveryDecision, JailerError>`
  reads any prior plan + state and emits one of:
  `LiveJail { jailer_pid, firecracker_pid }`, `OrphanJail { reap_steps }`,
  or `NoJail`. The crate does not act on the decision; the caller does.
- The chroot dir lives under `<run_dir>/jail/` (or a configured root).
  This crate does **not** decide where the run_dir is — that's the
  orchestrator's job.
- UID/GID for the jailed process are configurable. Defaults are recorded
  in the manifest, not hard-coded here.

## Public surface

- `JailerConfig { jailer_bin, firecracker_bin, run_dir, uid, gid,
  bindings: Vec<Binding>, sockets: Vec<SocketSpec> }`.
- `Binding { source: PathBuf, dest: PathBuf, mode: BindMode }` and
  `BindMode { Ro, Rw, CreateInsideJail }`.
- `Plan`, `MaterializedJail`, `JailedFirecracker`.
- `recover_from_run_dir`, `RecoveryDecision`.
- `JailerError`: `LaunchPrivilegeUnavailable`, `BindFailed`,
  `ChrootFailed`, `UidGidInvalid`,
  `Io { path: PathBuf, source: io::Error }`.
  The `Privilege(m80_privileged::PrivilegeError)` variant was removed:
  `m80-privileged` is not a workspace crate; privilege is acquired by the
  m80 process at startup and verified by `m80-preflight`.

## Non-goals

- **No cgroup configuration.** Jailer can do this, but m80 splits cgroup
  enforcement into `m80-cgroup` (which depends on `m80-jailer`).
- **No network namespace setup.** `m80-net-outbound` lives outside.
- **No process supervision after launch.** The orchestrator owns the
  child PIDs once `launch()` returns.
- **No general "make me a chroot" service.** This crate is shaped around
  Firecracker's jailer specifically.

## Dependencies

- `serde`, `serde_json` — for plan/state JSON.
- `thiserror`, `tracing`, `nix`.

## Tests

`tests/plan_compute.rs` — pure plan tests (no I/O):
- Determinism: two `Plan::compute` calls with the same config produce
  byte-equivalent JSON.
- Step ordering: `CreateInsideJail` dirs before `Bind`, sockets last.
- UID 0 or GID 0 rejected → `JailerError::UidGidInvalid`.

`tests/plan_serde.rs` — serde round-trips:
- `Plan` round-trips through `serde_json` with equal struct contents.
- Pretty-printed round-trip is byte-identical.
- Step kinds tagged as `create_dir`, `bind`, `socket` in JSON.
- `BindMode` serializes as `ro`, `rw`, `create_inside_jail`.

`tests/recover.rs` — fixture run-dir tests:
- No state file → `NoJail`.
- Stale state with non-existent pids → `OrphanJail` with reap steps.
- Self PID used as fixture pid (always alive) → `LiveJail`.
- Mixed alive/dead pids → `OrphanJail`.
- No plan file present → `OrphanJail` with empty reap steps.

`tests/integration_root.rs` — marked `#[ignore]` (requires root):
- `materialize` creates the jail root and writes plan/state JSON.
