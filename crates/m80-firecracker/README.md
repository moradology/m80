# `m80-firecracker`

The orchestrator. Composes every foundation crate into a usable sandbox:
preflight → boot → ready → run → stop → cleanup. Owns the lifecycle
state machine, the run-root layout, the admission semaphore, and the
configuration loading order.

## Reason for being

The 17 foundation/networking/observability crates are leaves with crisp
contracts. m80-firecracker is the one place where they compose. Without
it, every consumer of m80 would have to assemble the lifecycle by hand
— which is exactly what makes Firecracker hard to use straight out of
the box.

The crate is intentionally **glue plus a state machine**. Heavy logic
belongs in the foundation crates; this crate sequences calls and
manages the run-dir.

## Black-box contract

### Lifecycle state machine

A VM moves through four typed states:

```
Created → Running → Stopped → (Deleted | preserved-for-triage)
```

Each state is a distinct Rust type (`Sandbox`, `RunningSandbox`,
`StoppedSandbox`); transitions consume the prior handle so callers can't
double-stop or exec on a stopped VM. Force-kill collapses Running →
Stopped while preserving the run-dir for offline inspection.

- `Sandbox::new(config: SandboxConfig) -> Result<Sandbox, FcError>`
  produces a `Created` sandbox. No I/O happens yet — admission permit
  acquired but no chroot/mount/REST work has run.
- `Sandbox::launch() -> Result<RunningSandbox, FcError>` runs the strict
  preboot pipeline internally (preflight → run-root prep → lease
  acquisition → storage prep → jailer materialize → cgroup subtree →
  network realize → guest config injection → vsock → REST PUTs →
  InstanceStart → ready probe). The intermediate phases are not
  separate states — they're sub-steps of the single Created → Running
  transition. Each sub-step has its own typed error variant on
  `FcError`; failures surface the typed cause with phase context.
- `RunningSandbox::exec(req: ExecRequest) -> Result<ExecResponse, FcError>`
  opens an `m80-vsock::Channel` to the guest, sends the request,
  returns the response. One outstanding exec per sandbox in v0.1.
- `RunningSandbox::stop() -> Result<StoppedSandbox, FcError>` follows
  the four-phase teardown: admission_fence → bounded_stop (graceful
  on x86_64, forced on aarch64) → optional change-extract → release.
- `StoppedSandbox::extract_changes(into: &Path) -> Result<ChangeSet, FcError>`
  is the opt-in change-extraction step. Skipping it is fine; the
  scratch image is just discarded on delete.
- `StoppedSandbox::delete()` removes residue idempotently.
  `StoppedSandbox::preserve_for_triage()` keeps the run-dir intact and
  returns a path for offline inspection.

### Run-root layout invariants

- All per-VM state lives under `<run_root>/<vm_id>/`.
- A background `recover_stale_run_root()` task scans every 5 seconds
  and reaps orphans. It only touches subdirectories whose ownership
  markers are unambiguous; ambiguous residue is preserved.
- Cross-process collision avoidance: two m80 processes on the same host
  use distinct `<run_root>` paths, distinguished by `sha256(path)` in
  derived names (bridge, tap, etc.).

### Concurrency / admission

- `Backend::new(config: BackendConfig) -> Result<Backend, FcError>` is
  the long-lived handle a service holds. It wraps an `AdmissionLimited`
  semaphore sized by `M80_FIRECRACKER_MAX_CONCURRENT_VMS` (default
  derived from host capacity at preflight).
- All Sandbox creations go through `Backend::admit().launch()`; the
  admission permit is dropped on `delete()`.

### Configuration

- Loading order: built-in defaults → `/etc/m80/config.toml` →
  `~/.config/m80/config.toml` → environment (`M80_*` vars) → CLI
  flags (when invoked through `m80-cli`).
- `Backend::show_effective_config() -> EffectiveConfig` reveals the
  merged result for diagnostics.
- The `Backend` is constructed once per process and reused for every
  request.

### Error model

- All errors are typed variants of `FcError`. The variant tells the
  caller *which phase* failed; the inner cause carries phase-specific
  detail (e.g., `FcError::Preflight(PreflightError::KvmUnavailable)`).
- Every error includes "what to try next" hints renderable as CLI
  help text. `m80-cli` renders these on stderr.
- No silent degradation. Anything that would have to compromise an
  invariant fails closed.

## Public surface

- `SandboxConfig`, `Sandbox`, `RunningSandbox`, `StoppedSandbox`.
- `Backend`, `BackendConfig`, `EffectiveConfig`.
- `ExecRequest`, `ExecResponse`, `ExecStatus`.
- `FcError` — top-level error sum.

## Non-goals

- **No agent semantics.** No tool catalog, no `EffectClass`, no
  authority leases, no commit-authority, no idempotency contract. m80's
  job is "boot a VM and run a command"; agent semantics layer above.
- **No multi-host placement.** Single host only. Multi-host is the
  caller's concern.
- **No persistent VM pools.** Every `launch()` is a fresh boot in v0.1.
  Warm pools are a v0.2 epic.
- **No "execute and forget".** All sandboxes return through `stop()`
  or `force_kill()`; nothing is left running implicitly.

## Dependencies

The orchestrator pulls in essentially all foundation crates:

- `m80-preflight`, `m80-image-manifest`, `m80-firecracker-client`,
  `m80-jailer`, `m80-cgroup`, `m80-storage`, `m80-vsock`, `m80-net-mode`,
  `m80-net-outbound`, `m80-proto`.
- `m80-snapshot`, `m80-observability` (deferred surfaces).
- `serde`, `serde_json`, `thiserror`, `tracing`.

## Tests

- State-machine completeness: every documented edge in the lifecycle
  graph has a happy-path test and a failure-path test.
- Phase-error mapping: each phase in the preboot pipeline has a fixture
  that fails it; the resulting `FcError` carries the right inner type.
- Recovery: kill the orchestrator mid-launch, restart it, and verify
  `recover_stale_run_root` reaps the partial residue without disturbing
  a separately-running healthy sandbox.
- Configuration precedence: a multi-source config (defaults + file +
  env + flags) merges in the documented order.
- End-to-end (KVM-required, ignored by default): launch → exec → stop
  → extract_changes → delete a real Firecracker VM and verify the
  workspace mutation lands.
